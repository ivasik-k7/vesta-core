#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use {
    anchor_lang::{
        prelude::{Clock, Pubkey},
        solana_program::{
            instruction::{AccountMeta, Instruction},
            system_program,
        },
        AccountDeserialize, AnchorDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::associated_token::get_associated_token_address_with_program_id,
    argus::instructions::policy::{InitialPolicy, PolicyUpdate},
    litesvm::LiteSVM,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    spl_token_2022_interface::{
        extension::{transfer_hook::TransferHook, BaseStateWithExtensions, StateWithExtensions},
        state::Mint as MintState,
    },
    vesta_core::{
        constants::{CONFIG_SEED, CUSTOMER_SEED, MERCHANT_SEED, MINT_SEED},
        RegisterMerchantArgs,
    },
};

const TOKEN_2022_ID: Pubkey = spl_token_2022_interface::ID;
const ATA_PROGRAM_ID: Pubkey =
    Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const CAP: u64 = argus::constants::DEFAULT_DAILY_GIFT_CAP_RAW;

fn core_bytes() -> &'static [u8] {
    include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/vesta_core.so"
    ))
}
fn argus_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/argus.so"))
}
fn aegis_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/aegis.so"))
}

/// A policy with a plain daily cap and the program-owned filter on — the
/// v1-equivalent baseline most tests build from.
fn base_policy() -> InitialPolicy {
    InitialPolicy {
        flags: argus::constants::flags::BLOCK_PROGRAM_OWNED,
        daily_gift_cap: CAP,
        per_tx_cap: 0,
        max_wallet_balance: 0,
        transfers_per_day_cap: 0,
        cooldown_secs: 0,
        aegis_program: Pubkey::default(),
        policy: Pubkey::default(),
        attestation_issuer: Pubkey::default(),
        attestation_schema: 0,
        capability_ttl_secs: 0,
    }
}

struct World {
    svm: LiteSVM,
    #[allow(dead_code)]
    admin: Keypair,
    config: Pubkey,
    merchant_authority: Keypair,
    merchant: Pubkey,
    mint: Pubkey,
    treasury: Pubkey,
    eaml: Pubkey,
}

impl World {
    fn send(
        &mut self,
        ixs: &[Instruction],
        signers: &[&Keypair],
        payer: &Pubkey,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let mut msg = Message::new(ixs, Some(payer));
        msg.recent_blockhash = self.svm.latest_blockhash();
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
        let result = self.svm.send_transaction(tx);
        // Always advance the blockhash — even after a failed send — so an
        // identical retry gets a fresh signature instead of AlreadyProcessed.
        self.svm.expire_blockhash();
        result.map(|_| ()).map_err(Box::new)
    }

    fn warp_secs(&mut self, secs: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp += secs;
        self.svm.set_sysvar::<Clock>(&clock);
    }

    fn warp_days(&mut self, days: i64) {
        self.warp_secs(days * 86_400);
    }

    fn earn(&mut self, customer: Pubkey, amount_base: u64) {
        let visit_day = (self.svm.get_sysvar::<Clock>().unix_timestamp / 86_400) as u32;
        let profile = Pubkey::find_program_address(
            &[CUSTOMER_SEED, self.merchant.as_ref(), customer.as_ref()],
            &vesta_core::id(),
        )
        .0;
        let ata =
            get_associated_token_address_with_program_id(&customer, &self.mint, &TOKEN_2022_ID);
        let authority = self.merchant_authority.insecure_clone();
        let ix = Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::EarnPoints {
                merchant_authority: authority.pubkey(),
                merchant: self.merchant,
                customer,
                customer_profile: profile,
                point_mint: self.mint,
                customer_ata: ata,
                config: self.config,
                token_program: TOKEN_2022_ID,
                associated_token_program: ATA_PROGRAM_ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::EarnPoints {
                amount_base,
                visit_day,
            }
            .data(),
        };
        self.send(&[ix], &[&authority], &authority.pubkey())
            .unwrap();
    }

    fn guard_config(&self) -> Pubkey {
        Pubkey::find_program_address(&[b"guard", self.mint.as_ref()], &argus::id()).0
    }

    fn guard_init_ix(
        &self,
        signer: Pubkey,
        merchant: Pubkey,
        mint: Pubkey,
        policy: InitialPolicy,
    ) -> Instruction {
        Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::InitializeTransferGuard {
                merchant_authority: signer,
                merchant,
                mint,
                guard_config: Pubkey::find_program_address(
                    &[b"guard", mint.as_ref()],
                    &argus::id(),
                )
                .0,
                extra_account_meta_list: Pubkey::find_program_address(
                    &[b"extra-account-metas", mint.as_ref()],
                    &argus::id(),
                )
                .0,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: argus::instruction::InitializeTransferGuard { policy }.data(),
        }
    }

    fn configure(&mut self, update: PolicyUpdate) {
        let authority = self.merchant_authority.insecure_clone();
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::GuardAuthorityOnly {
                authority: authority.pubkey(),
                guard_config: self.guard_config(),
            }
            .to_account_metas(None),
            data: argus::instruction::ConfigurePolicy { update }.data(),
        };
        self.send(&[ix], &[&authority], &authority.pubkey())
            .unwrap();
    }

    fn set_paused(&mut self, paused: bool) {
        let authority = self.merchant_authority.insecure_clone();
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::GuardAuthorityOnly {
                authority: authority.pubkey(),
                guard_config: self.guard_config(),
            }
            .to_account_metas(None),
            data: argus::instruction::SetGuardPaused { paused }.data(),
        };
        self.send(&[ix], &[&authority], &authority.pubkey())
            .unwrap();
    }

    fn add_list_entry(&mut self, target: Pubkey) {
        let authority = self.merchant_authority.insecure_clone();
        let entry = Pubkey::find_program_address(
            &[b"entry", self.mint.as_ref(), target.as_ref()],
            &argus::id(),
        )
        .0;
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::AddListEntry {
                authority: authority.pubkey(),
                guard_config: self.guard_config(),
                entry,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: argus::instruction::AddListEntry { target }.data(),
        };
        self.send(&[ix], &[&authority], &authority.pubkey())
            .unwrap();
    }

    fn open_state(&mut self, owner: &Keypair) -> Pubkey {
        let state = Pubkey::find_program_address(
            &[b"wstate", self.mint.as_ref(), owner.pubkey().as_ref()],
            &argus::id(),
        )
        .0;
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::OpenWalletState {
                owner: owner.pubkey(),
                mint: self.mint,
                wallet_state: state,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: argus::instruction::OpenWalletState {}.data(),
        };
        self.send(&[ix], &[owner], &owner.pubkey()).unwrap();
        state
    }

    /// The argus hook extras (meta-list order) + argus program + eaml, ready to
    /// append to a transfer_checked. `issuer` is the attestation issuer pinned
    /// at guard init (default when attestation is unused).
    fn hook_extras(
        &self,
        source_owner: Pubkey,
        dest_wallet: Pubkey,
        _issuer: Pubkey,
    ) -> Vec<AccountMeta> {
        let g = |seeds: &[&[u8]]| Pubkey::find_program_address(seeds, &argus::id()).0;
        let guard_config = g(&[b"guard", self.mint.as_ref()]);
        let wallet_state = g(&[b"wstate", self.mint.as_ref(), source_owner.as_ref()]);
        let list_entry = g(&[b"entry", self.mint.as_ref(), dest_wallet.as_ref()]);
        // v2: the aegis program/issuer/attestation trio is replaced by argus's
        // own cached EligibilityCapability for the destination owner (spec 09).
        let capability = g(&[b"cap", self.mint.as_ref(), dest_wallet.as_ref()]);
        vec![
            AccountMeta::new_readonly(guard_config, false),
            AccountMeta::new(wallet_state, false),
            AccountMeta::new_readonly(dest_wallet, false),
            AccountMeta::new_readonly(list_entry, false),
            AccountMeta::new_readonly(capability, false),
            AccountMeta::new_readonly(argus::id(), false),
            AccountMeta::new_readonly(self.eaml, false),
        ]
    }

    /// Derive an argus EligibilityCapability PDA for (mint, subject).
    fn capability_pda(&self, subject: Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[b"cap", self.mint.as_ref(), subject.as_ref()],
            &argus::id(),
        )
        .0
    }

    /// Refresh a subject's eligibility capability by CPI-ing aegis `verify`.
    fn refresh_eligibility(
        &mut self,
        payer: &Keypair,
        subject: Pubkey,
        issuer: Pubkey,
        schema_id: u64,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let attestation = Pubkey::find_program_address(
            &[
                b"attestation",
                issuer.as_ref(),
                subject.as_ref(),
                &schema_id.to_le_bytes(),
            ],
            &aegis::id(),
        )
        .0;
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::RefreshEligibility {
                payer: payer.pubkey(),
                mint: self.mint,
                guard_config: Pubkey::find_program_address(
                    &[b"guard", self.mint.as_ref()],
                    &argus::id(),
                )
                .0,
                subject,
                capability: self.capability_pda(subject),
                attestation,
                // Present-path helper: the guard configures no aegis Policy, so
                // this account is ignored — pass the program id as a placeholder.
                policy: aegis::id(),
                aegis_program: aegis::id(),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: argus::instruction::RefreshEligibility {}.data(),
        };
        self.send(&[ix], &[payer], &payer.pubkey())
    }

    /// Guard-authority: force-invalidate a subject's cached capability.
    fn invalidate_capability(
        &mut self,
        subject: Pubkey,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let authority = self.merchant_authority.insecure_clone();
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::InvalidateCapability {
                authority: authority.pubkey(),
                mint: self.mint,
                guard_config: Pubkey::find_program_address(
                    &[b"guard", self.mint.as_ref()],
                    &argus::id(),
                )
                .0,
                subject,
                capability: self.capability_pda(subject),
            }
            .to_account_metas(None),
            data: argus::instruction::InvalidateCapability {}.data(),
        };
        self.send(&[ix], &[&authority], &authority.pubkey())
    }

    #[allow(clippy::too_many_arguments)]
    fn hooked_transfer_ix(
        &self,
        source_owner: Pubkey,
        authority: Pubkey,
        destination_wallet: Pubkey,
        amount: u64,
        issuer: Pubkey,
        with_extras: bool,
    ) -> Instruction {
        let source =
            get_associated_token_address_with_program_id(&source_owner, &self.mint, &TOKEN_2022_ID);
        let destination = get_associated_token_address_with_program_id(
            &destination_wallet,
            &self.mint,
            &TOKEN_2022_ID,
        );
        let mut ix = spl_token_2022_interface::instruction::transfer_checked(
            &TOKEN_2022_ID,
            &source,
            &self.mint,
            &destination,
            &authority,
            &[],
            amount,
            2,
        )
        .unwrap();
        if with_extras {
            ix.accounts
                .extend(self.hook_extras(source_owner, destination_wallet, issuer));
        }
        ix
    }

    fn create_ata_ix(&self, owner: Pubkey, payer: Pubkey) -> Instruction {
        spl_associated_token_account_interface::instruction::create_associated_token_account_idempotent(
            &payer,
            &owner,
            &self.mint,
            &TOKEN_2022_ID,
        )
    }

    fn state_of(&self, owner: Pubkey) -> argus::state::WalletPolicyState {
        let state = Pubkey::find_program_address(
            &[b"wstate", self.mint.as_ref(), owner.as_ref()],
            &argus::id(),
        )
        .0;
        let data = self.svm.get_account(&state).unwrap().data;
        argus::state::WalletPolicyState::try_deserialize(&mut data.as_slice()).unwrap()
    }

    fn config_of(&self) -> argus::state::GuardConfig {
        let data = self.svm.get_account(&self.guard_config()).unwrap().data;
        argus::state::GuardConfig::try_deserialize(&mut data.as_slice()).unwrap()
    }

    // ── Governance (spec 10) helpers ─────────────────────────────────────────

    fn roles_pda(&self) -> Pubkey {
        Pubkey::find_program_address(&[b"roles", self.mint.as_ref()], &argus::id()).0
    }

    fn pointer_pda(&self) -> Pubkey {
        Pubkey::find_program_address(&[b"active", self.mint.as_ref()], &argus::id()).0
    }

    fn version_pda(&self, hash: &[u8; 32]) -> Pubkey {
        Pubkey::find_program_address(&[b"pver", self.mint.as_ref(), hash], &argus::id()).0
    }

    fn pointer_of(&self) -> argus::state::PolicyPointer {
        let data = self.svm.get_account(&self.pointer_pda()).unwrap().data;
        argus::state::PolicyPointer::try_deserialize(&mut data.as_slice()).unwrap()
    }

    fn roles_of(&self) -> argus::state::RoleRegistry {
        let data = self.svm.get_account(&self.roles_pda()).unwrap().data;
        argus::state::RoleRegistry::try_deserialize(&mut data.as_slice()).unwrap()
    }

    /// Adopt governance with the given role authorities and timelock.
    #[allow(clippy::too_many_arguments)]
    fn init_governance(
        &mut self,
        roles: argus::instructions::governance::RoleAssignment,
        timelock_secs: i64,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let authority = self.merchant_authority.insecure_clone();
        let genesis = self.config_of().as_policy_doc();
        let gh = genesis.hash().unwrap();
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::InitializeGovernance {
                authority: authority.pubkey(),
                mint: self.mint,
                guard_config: self.guard_config(),
                role_registry: self.roles_pda(),
                policy_pointer: self.pointer_pda(),
                genesis_version: self.version_pda(&gh),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: argus::instruction::InitializeGovernance {
                genesis_hash: gh,
                roles,
                timelock_secs,
            }
            .data(),
        };
        self.send(&[ix], &[&authority], &authority.pubkey())
    }

    fn propose_policy(
        &mut self,
        author: &Keypair,
        doc: argus::state::PolicyDoc,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let hash = doc.hash().unwrap();
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::ProposePolicy {
                author: author.pubkey(),
                role_registry: self.roles_pda(),
                policy_pointer: self.pointer_pda(),
                policy_version: self.version_pda(&hash),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: argus::instruction::ProposePolicy {
                policy_hash: hash,
                doc,
            }
            .data(),
        };
        self.send(&[ix], &[author], &author.pubkey())
    }

    fn approve_policy(
        &mut self,
        approver: &Keypair,
        hash: [u8; 32],
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::ApprovePolicy {
                approver: approver.pubkey(),
                role_registry: self.roles_pda(),
                policy_pointer: self.pointer_pda(),
                policy_version: self.version_pda(&hash),
            }
            .to_account_metas(None),
            data: argus::instruction::ApprovePolicy {}.data(),
        };
        self.send(&[ix], &[approver], &approver.pubkey())
    }

    fn activate_or_rollback(
        &mut self,
        activator: &Keypair,
        hash: [u8; 32],
        rollback: bool,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let accounts = argus::accounts::ActivatePolicy {
            activator: activator.pubkey(),
            role_registry: self.roles_pda(),
            policy_pointer: self.pointer_pda(),
            policy_version: self.version_pda(&hash),
            guard_config: self.guard_config(),
        }
        .to_account_metas(None);
        let data = if rollback {
            argus::instruction::RollbackPolicy {}.data()
        } else {
            argus::instruction::ActivatePolicy {}.data()
        };
        let ix = Instruction {
            program_id: argus::id(),
            accounts,
            data,
        };
        self.send(&[ix], &[activator], &activator.pubkey())
    }

    fn statement_pda(&self, period: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[b"statement", self.mint.as_ref(), &period.to_le_bytes()],
            &argus::id(),
        )
        .0
    }

    fn anchor_statement(
        &mut self,
        reporter: &Keypair,
        period: u64,
        root: [u8; 32],
        count: u64,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::AnchorStatement {
                reporter: reporter.pubkey(),
                mint: self.mint,
                role_registry: self.roles_pda(),
                statement: self.statement_pda(period),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: argus::instruction::AnchorStatement {
                period,
                merkle_root: root,
                decision_count: count,
            }
            .data(),
        };
        self.send(&[ix], &[reporter], &reporter.pubkey())
    }

    fn statement_of(&self, period: u64) -> argus::state::StatementCommitment {
        let data = self
            .svm
            .get_account(&self.statement_pda(period))
            .unwrap()
            .data;
        argus::state::StatementCommitment::try_deserialize(&mut data.as_slice()).unwrap()
    }

    // ── Trust triangle (spec 10 §4.3) helpers ────────────────────────────────

    fn trust_anchor_pda(&self) -> Pubkey {
        Pubkey::find_program_address(&[b"trust", self.mint.as_ref()], &argus::id()).0
    }

    fn set_trust_anchor(
        &mut self,
        root: Pubkey,
        subject_issuer: Pubkey,
        schema: u64,
        target: u8,
        grace: i64,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let authority = self.merchant_authority.insecure_clone();
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::SetTrustAnchor {
                authority: authority.pubkey(),
                mint: self.mint,
                guard_config: self.guard_config(),
                trust_anchor: self.trust_anchor_pda(),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: argus::instruction::SetTrustAnchor {
                accreditation_root: root,
                subject_issuer,
                required_schema: schema,
                degrade_target: target,
                grace_secs: grace,
            }
            .data(),
        };
        self.send(&[ix], &[&authority], &authority.pubkey())
    }

    fn reverify(
        &mut self,
        cranker: &Keypair,
        root: Pubkey,
        subject_issuer: Pubkey,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let aegis_trust_root =
            Pubkey::find_program_address(&[b"troot", root.as_ref()], &aegis::id()).0;
        let aegis_accreditation = Pubkey::find_program_address(
            &[b"accred", root.as_ref(), subject_issuer.as_ref()],
            &aegis::id(),
        )
        .0;
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::ReverifyAccreditation {
                cranker: cranker.pubkey(),
                mint: self.mint,
                guard_config: self.guard_config(),
                trust_anchor: self.trust_anchor_pda(),
                aegis_trust_root,
                aegis_accreditation,
                aegis_program: aegis::id(),
            }
            .to_account_metas(None),
            data: argus::instruction::ReverifyAccreditation {}.data(),
        };
        self.send(&[ix], &[cranker], &cranker.pubkey())
    }

    fn set_degrade(
        &mut self,
        mode: u8,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let authority = self.merchant_authority.insecure_clone();
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::SetDegradeMode {
                authority: authority.pubkey(),
                mint: self.mint,
                guard_config: self.guard_config(),
                trust_anchor: self.trust_anchor_pda(),
            }
            .to_account_metas(None),
            data: argus::instruction::SetDegradeMode { mode }.data(),
        };
        self.send(&[ix], &[&authority], &authority.pubkey())
    }
}

fn setup() -> World {
    let mut svm = LiteSVM::new();
    svm.add_program(vesta_core::id(), core_bytes()).unwrap();
    svm.add_program(argus::id(), argus_bytes()).unwrap();
    svm.add_program(aegis::id(), aegis_bytes()).unwrap();
    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 1_760_000_000;
    svm.set_sysvar::<Clock>(&clock);

    let admin = Keypair::new();
    svm.airdrop(&admin.pubkey(), 100_000_000_000).unwrap();
    let config = Pubkey::find_program_address(&[CONFIG_SEED], &vesta_core::id()).0;

    let merchant_authority = Keypair::new();
    svm.airdrop(&merchant_authority.pubkey(), 50_000_000_000)
        .unwrap();
    let merchant = Pubkey::find_program_address(
        &[
            MERCHANT_SEED,
            merchant_authority.pubkey().as_ref(),
            &0u64.to_le_bytes(),
        ],
        &vesta_core::id(),
    )
    .0;
    let mint = Pubkey::find_program_address(&[MINT_SEED, merchant.as_ref()], &vesta_core::id()).0;
    let treasury = get_associated_token_address_with_program_id(
        &merchant_authority.pubkey(),
        &mint,
        &TOKEN_2022_ID,
    );
    let eaml =
        Pubkey::find_program_address(&[b"extra-account-metas", mint.as_ref()], &argus::id()).0;

    let mut w = World {
        svm,
        admin,
        config,
        merchant_authority,
        merchant,
        mint,
        treasury,
        eaml,
    };

    let admin = w.admin.insecure_clone();
    w.send(
        &[Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::InitConfig {
                admin: admin.pubkey(),
                config: w.config,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::InitConfig {}.data(),
        }],
        &[&admin],
        &admin.pubkey(),
    )
    .unwrap();

    let authority = w.merchant_authority.insecure_clone();
    w.send(
        &[Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::RegisterMerchant {
                authority: authority.pubkey(),
                merchant: w.merchant,
                mint: w.mint,
                treasury: w.treasury,
                config: w.config,
                token_program: TOKEN_2022_ID,
                associated_token_program: ATA_PROGRAM_ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::RegisterMerchant {
                id: 0,
                args: RegisterMerchantArgs {
                    name: "Kavarna".into(),
                    symbol: "PTS".into(),
                    uri: "https://vesta.example/points.json".into(),
                    decay_rate_bps: -2_000,
                    base_earn_rate: 100,
                    decimals: 2,
                },
            }
            .data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    w
}

fn setup_with_policy(policy: InitialPolicy) -> World {
    let mut w = setup();
    let authority = w.merchant_authority.insecure_clone();
    let ix = w.guard_init_ix(authority.pubkey(), w.merchant, w.mint, policy);
    w.send(&[ix], &[&authority], &authority.pubkey()).unwrap();
    w
}

fn setup_with_guard() -> World {
    setup_with_policy(base_policy())
}

/// Fund a wallet with points, create the recipient ATA, and open velocity state.
fn prime_sender(w: &mut World, sender: &Keypair, recipient: Pubkey, base: u64) {
    w.svm.airdrop(&sender.pubkey(), 10_000_000_000).unwrap();
    w.earn(sender.pubkey(), base);
    let ata_ix = w.create_ata_ix(recipient, sender.pubkey());
    w.send(&[ata_ix], &[sender], &sender.pubkey()).unwrap();
    w.open_state(sender);
}

#[test]
fn guard_init_requires_the_merchant_authority() {
    let mut w = setup();
    let rando = Keypair::new();
    w.svm.airdrop(&rando.pubkey(), 5_000_000_000).unwrap();
    let ix = w.guard_init_ix(rando.pubkey(), w.merchant, w.mint, base_policy());
    assert!(
        w.send(&[ix], &[&rando], &rando.pubkey()).is_err(),
        "front-runner initialized the guard"
    );
}

#[test]
fn guard_init_writes_config_and_rejects_double_init() {
    let mut w = setup();
    let authority = w.merchant_authority.insecure_clone();
    let ix = w.guard_init_ix(authority.pubkey(), w.merchant, w.mint, base_policy());
    w.send(
        std::slice::from_ref(&ix),
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();

    let cfg = w.config_of();
    assert_eq!(cfg.daily_gift_cap, CAP);
    assert_eq!(cfg.treasury, w.treasury);
    assert_eq!(cfg.authority, authority.pubkey());
    assert_eq!(cfg.flags, argus::constants::flags::BLOCK_PROGRAM_OWNED);

    assert!(
        w.send(&[ix], &[&authority], &authority.pubkey()).is_err(),
        "double guard init accepted"
    );
}

#[test]
fn finalize_burns_the_hook_authority_exactly_once() {
    let mut w = setup();
    let authority = w.merchant_authority.insecure_clone();

    let finalize = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::FinalizeTransferGuard {
            authority: authority.pubkey(),
            merchant: w.merchant,
            point_mint: w.mint,
            extra_account_meta_list: w.eaml,
            config: w.config,
            token_program: TOKEN_2022_ID,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::FinalizeTransferGuard {}.data(),
    };

    assert!(w
        .send(
            std::slice::from_ref(&finalize),
            &[&authority],
            &authority.pubkey()
        )
        .is_err());

    let init = w.guard_init_ix(authority.pubkey(), w.merchant, w.mint, base_policy());
    w.send(&[init], &[&authority], &authority.pubkey()).unwrap();

    w.send(
        std::slice::from_ref(&finalize),
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();

    let mint_data = w.svm.get_account(&w.mint).unwrap().data;
    let state = StateWithExtensions::<MintState>::unpack(&mint_data).unwrap();
    let hook = state.get_extension::<TransferHook>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(hook.authority),
        None,
        "hook authority not burned"
    );
    assert_eq!(
        Option::<Pubkey>::from(hook.program_id),
        Some(argus::id()),
        "hook program must stay pinned to argus"
    );

    assert!(w
        .send(
            std::slice::from_ref(&finalize),
            &[&authority],
            &authority.pubkey()
        )
        .is_err());
}

#[test]
fn gift_flow_enforces_cap_rollover_and_fail_closed() {
    let mut w = setup_with_guard();
    let alice = Keypair::new();
    let bob = Keypair::new();
    let d = Pubkey::default();
    w.svm.airdrop(&alice.pubkey(), 10_000_000_000).unwrap();
    w.earn(alice.pubkey(), 5_000); // 510_000 raw at 1.02x
    let ata_ix = w.create_ata_ix(bob.pubkey(), alice.pubkey());
    w.send(&[ata_ix], &[&alice], &alice.pubkey()).unwrap();

    // Fail-closed: no wallet state opened yet.
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, d, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "stateless gift passed"
    );

    // Fail-closed: extras omitted entirely.
    w.open_state(&alice);
    let bare = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        d,
        false,
    );
    assert!(
        w.send(&[bare], &[&alice], &alice.pubkey()).is_err(),
        "hookless transfer passed"
    );

    // Within cap passes and the state tracks it.
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        30_000,
        d,
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.state_of(alice.pubkey()).sent_today, 30_000);
    assert_eq!(w.state_of(alice.pubkey()).transfers_today, 1);

    // Exactly to the cap passes; one more unit fails.
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        CAP - 30_000,
        d,
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.state_of(alice.pubkey()).sent_today, CAP);
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1, d, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "cap breach passed"
    );

    // Day rollover resets the window.
    w.warp_days(1);
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, d, true);
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.state_of(alice.pubkey()).sent_today, 1_000);
}

#[test]
fn treasury_payments_bypass_the_cap_without_state() {
    let mut w = setup_with_guard();
    let alice = Keypair::new();
    w.svm.airdrop(&alice.pubkey(), 10_000_000_000).unwrap();
    w.earn(alice.pubkey(), 5_000);

    // No state opened, amount far above the gift cap — rule 2 short-circuits.
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        w.merchant_authority.pubkey(),
        CAP * 4,
        Pubkey::default(),
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
}

#[test]
fn delegated_transfers_spend_the_source_owners_state() {
    let mut w = setup_with_guard();
    let alice = Keypair::new();
    let bob = Keypair::new();
    let delegate = Keypair::new();
    let d = Pubkey::default();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);
    w.svm.airdrop(&delegate.pubkey(), 5_000_000_000).unwrap();

    let alice_ata =
        get_associated_token_address_with_program_id(&alice.pubkey(), &w.mint, &TOKEN_2022_ID);
    let approve = spl_token_2022_interface::instruction::approve(
        &TOKEN_2022_ID,
        &alice_ata,
        &delegate.pubkey(),
        &alice.pubkey(),
        &[],
        40_000,
    )
    .unwrap();
    w.send(&[approve], &[&alice], &alice.pubkey()).unwrap();

    // The delegate signs, but the state is derived from ALICE's owner field —
    // a delegate cannot mint themselves a fresh cap.
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        delegate.pubkey(),
        bob.pubkey(),
        25_000,
        d,
        true,
    );
    w.send(&[ix], &[&delegate], &delegate.pubkey()).unwrap();
    assert_eq!(w.state_of(alice.pubkey()).sent_today, 25_000);
}

#[test]
fn program_owned_destinations_are_rejected_when_flagged() {
    let mut w = setup_with_guard();
    let alice = Keypair::new();
    w.svm.airdrop(&alice.pubkey(), 10_000_000_000).unwrap();
    w.earn(alice.pubkey(), 5_000);
    w.open_state(&alice);

    // The EAML account is owned by argus — a stand-in for a pool/vault authority.
    let pool_authority = w.eaml;
    let ata_ix = w.create_ata_ix(pool_authority, alice.pubkey());
    w.send(&[ata_ix], &[&alice], &alice.pubkey()).unwrap();

    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        pool_authority,
        1_000,
        Pubkey::default(),
        true,
    );
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "program-owned destination accepted"
    );
}

#[test]
fn per_tx_cap_and_balance_cap_bound_single_transfers() {
    let mut policy = base_policy();
    policy.per_tx_cap = 10_000;
    policy.max_wallet_balance = 15_000;
    let mut w = setup_with_policy(policy);
    let alice = Keypair::new();
    let bob = Keypair::new();
    let d = Pubkey::default();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);

    // Above per-tx cap → rejected.
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        10_001,
        d,
        true,
    );
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "per-tx breach passed"
    );

    // Balance cap is measured post-transfer. Bring bob to 10_000 (ok).
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        10_000,
        d,
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    // +6_000 would push bob to 16_000 > 15_000 → rejected.
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 6_000, d, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "balance cap breach passed"
    );
    // +5_000 keeps bob at 15_000 exactly — allowed.
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 5_000, d, true);
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
}

#[test]
fn cooldown_and_transfer_count_throttle_bursts() {
    let mut policy = base_policy();
    policy.cooldown_secs = 3_600;
    policy.transfers_per_day_cap = 2;
    let mut w = setup_with_policy(policy);
    let alice = Keypair::new();
    let bob = Keypair::new();
    let d = Pubkey::default();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);

    // Distinct amounts keep transaction signatures unique across sends.
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, d, true);
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    // Immediate second transfer → cooldown.
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_100, d, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "cooldown bypassed"
    );

    w.warp_secs(3_600);
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_200, d, true);
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.state_of(alice.pubkey()).transfers_today, 2);

    // Third within the day → count cap (cooldown elapsed).
    w.warp_secs(3_600);
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_300, d, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "count cap bypassed"
    );
}

#[test]
fn pause_blocks_peer_but_not_treasury() {
    let mut w = setup_with_guard();
    let alice = Keypair::new();
    let bob = Keypair::new();
    let d = Pubkey::default();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);
    w.set_paused(true);

    let peer = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, d, true);
    assert!(
        w.send(&[peer], &[&alice], &alice.pubkey()).is_err(),
        "paused peer transfer passed"
    );

    // Treasury payment still flows (rule 2 precedes the pause check).
    let pay = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        w.merchant_authority.pubkey(),
        1_000,
        d,
        true,
    );
    w.send(&[pay], &[&alice], &alice.pubkey()).unwrap();
}

#[test]
fn allowlist_only_gates_destinations() {
    let mut policy = base_policy();
    policy.flags |= argus::constants::flags::ALLOWLIST_ONLY;
    let mut w = setup_with_policy(policy);
    let alice = Keypair::new();
    let bob = Keypair::new();
    let d = Pubkey::default();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);

    // Bob is not listed → rejected.
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, d, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "unlisted destination passed"
    );

    // List bob → now allowed.
    w.add_list_entry(bob.pubkey());
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, d, true);
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.state_of(alice.pubkey()).sent_today, 1_000);
}

#[test]
fn denylist_blocks_listed_destinations() {
    let mut policy = base_policy();
    policy.flags |= argus::constants::flags::DENYLIST;
    let mut w = setup_with_policy(policy);
    let alice = Keypair::new();
    let bob = Keypair::new();
    let d = Pubkey::default();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);

    // Unlisted → allowed.
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, d, true);
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();

    // Deny bob → blocked.
    w.add_list_entry(bob.pubkey());
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, d, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "deny-listed destination passed"
    );
}

#[test]
fn configure_policy_retunes_the_cap() {
    let mut w = setup_with_guard();
    let alice = Keypair::new();
    let bob = Keypair::new();
    let d = Pubkey::default();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);

    // Tighten the daily cap to 500 raw.
    w.configure(PolicyUpdate {
        daily_gift_cap: Some(500),
        ..Default::default()
    });
    assert_eq!(w.config_of().daily_gift_cap, 500);

    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 501, d, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "retuned cap not enforced"
    );
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 500, d, true);
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
}

#[test]
fn two_step_authority_rotation() {
    let mut w = setup_with_guard();
    let old = w.merchant_authority.insecure_clone();
    let next = Keypair::new();
    w.svm.airdrop(&next.pubkey(), 5_000_000_000).unwrap();

    let propose = Instruction {
        program_id: argus::id(),
        accounts: argus::accounts::GuardAuthorityOnly {
            authority: old.pubkey(),
            guard_config: w.guard_config(),
        }
        .to_account_metas(None),
        data: argus::instruction::TransferGuardAuthority {
            new_authority: next.pubkey(),
        }
        .data(),
    };
    w.send(&[propose], &[&old], &old.pubkey()).unwrap();

    let accept = Instruction {
        program_id: argus::id(),
        accounts: argus::accounts::AcceptGuardAuthority {
            pending_authority: next.pubkey(),
            guard_config: w.guard_config(),
        }
        .to_account_metas(None),
        data: argus::instruction::AcceptGuardAuthority {}.data(),
    };
    w.send(&[accept], &[&next], &next.pubkey()).unwrap();
    assert_eq!(w.config_of().authority, next.pubkey());
}

/// The synergy test: aegis issues a region attestation that argus gates on.
#[test]
fn attestation_gating_composes_with_aegis() {
    let issuer_authority = Keypair::new();
    let issuer = Pubkey::find_program_address(
        &[
            b"issuer",
            issuer_authority.pubkey().as_ref(),
            &0u64.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;

    let mut policy = base_policy();
    policy.flags |= argus::constants::flags::REQUIRE_ATTESTATION;
    policy.aegis_program = aegis::id();
    policy.attestation_issuer = issuer;
    policy.attestation_schema = aegis::constants::well_known_schema::REGION;

    let mut w = setup_with_policy(policy);
    w.svm
        .airdrop(&issuer_authority.pubkey(), 5_000_000_000)
        .unwrap();

    let alice = Keypair::new();
    let bob = Keypair::new();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);

    // Init the aegis issuer.
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::InitIssuer {
                authority: issuer_authority.pubkey(),
                issuer,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::InitIssuer {
                id: 0,
                name: "GeoOracle".into(),
            }
            .data(),
        }],
        &[&issuer_authority],
        &issuer_authority.pubkey(),
    )
    .unwrap();

    let schema_id = aegis::constants::well_known_schema::REGION;
    let attestation = Pubkey::find_program_address(
        &[
            b"attestation",
            issuer.as_ref(),
            bob.pubkey().as_ref(),
            &schema_id.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;

    // No credential / capability on bob yet → gift rejected (fail closed).
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "gift passed without eligibility"
    );

    // Refreshing before any credential exists → aegis returns a negative
    // verdict → capability minted with the bit UNSET → still rejected.
    w.refresh_eligibility(&alice, bob.pubkey(), issuer, schema_id)
        .unwrap();
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "gift passed with an empty (negative) capability"
    );

    // Issue bob a region credential (commitment model — no plaintext on-chain).
    let issue = Instruction {
        program_id: aegis::id(),
        accounts: aegis::accounts::IssueAttestation {
            signer: issuer_authority.pubkey(),
            issuer,
            attestation,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: aegis::instruction::IssueAttestation {
            subject: bob.pubkey(),
            data: aegis::instructions::attestation::AttestationData {
                schema_id,
                commitment: [7u8; 32],
                attr_root: [0u8; 32],
                valid_from: 0,
                expires_at: 0,
            },
        }
        .data(),
    };
    w.send(&[issue], &[&issuer_authority], &issuer_authority.pubkey())
        .unwrap();

    // Still stale until refreshed — the capability predates the credential.
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "gift passed on a stale capability"
    );

    // Refresh → aegis `verify` now returns ok → capability bit set → allowed.
    w.refresh_eligibility(&alice, bob.pubkey(), issuer, schema_id)
        .unwrap();
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.state_of(alice.pubkey()).sent_today, 1_000);

    // Revoke the credential + refresh → verdict flips negative → gate closes
    // (fresh day to dodge the daily cap).
    let revoke = Instruction {
        program_id: aegis::id(),
        accounts: aegis::accounts::ManageAttestation {
            signer: issuer_authority.pubkey(),
            issuer,
            attestation,
        }
        .to_account_metas(None),
        data: aegis::instruction::RevokeAttestation { reason_code: 1 }.data(),
    };
    w.send(&[revoke], &[&issuer_authority], &issuer_authority.pubkey())
        .unwrap();
    w.warp_days(1);
    w.refresh_eligibility(&alice, bob.pubkey(), issuer, schema_id)
        .unwrap();
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "revoked credential still passed"
    );
}

/// The guard authority can close a subject's aegis-revocation-latency window
/// immediately via `invalidate_capability`, without a global epoch bump — and
/// a stranger cannot (`has_one = authority`).
#[test]
fn argus_invalidate_capability_closes_the_gate() {
    let issuer_authority = Keypair::new();
    let issuer = Pubkey::find_program_address(
        &[
            b"issuer",
            issuer_authority.pubkey().as_ref(),
            &0u64.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;

    let mut policy = base_policy();
    policy.flags |= argus::constants::flags::REQUIRE_ATTESTATION;
    policy.aegis_program = aegis::id();
    policy.attestation_issuer = issuer;
    policy.attestation_schema = aegis::constants::well_known_schema::REGION;

    let mut w = setup_with_policy(policy);
    w.svm
        .airdrop(&issuer_authority.pubkey(), 5_000_000_000)
        .unwrap();

    let alice = Keypair::new();
    let bob = Keypair::new();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);

    // Init the aegis issuer.
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::InitIssuer {
                authority: issuer_authority.pubkey(),
                issuer,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::InitIssuer {
                id: 0,
                name: "GeoOracle".into(),
            }
            .data(),
        }],
        &[&issuer_authority],
        &issuer_authority.pubkey(),
    )
    .unwrap();

    let schema_id = aegis::constants::well_known_schema::REGION;
    let attestation = Pubkey::find_program_address(
        &[
            b"attestation",
            issuer.as_ref(),
            bob.pubkey().as_ref(),
            &schema_id.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;

    // Issue a credential + refresh → capability bit set → gift allowed.
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::IssueAttestation {
                signer: issuer_authority.pubkey(),
                issuer,
                attestation,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::IssueAttestation {
                subject: bob.pubkey(),
                data: aegis::instructions::attestation::AttestationData {
                    schema_id,
                    commitment: [7u8; 32],
                    attr_root: [0u8; 32],
                    valid_from: 0,
                    expires_at: 0,
                },
            }
            .data(),
        }],
        &[&issuer_authority],
        &issuer_authority.pubkey(),
    )
    .unwrap();
    w.refresh_eligibility(&alice, bob.pubkey(), issuer, schema_id)
        .unwrap();
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();

    // A stranger cannot invalidate (guard authority only).
    let stranger = Keypair::new();
    w.svm.airdrop(&stranger.pubkey(), 5_000_000_000).unwrap();
    let bad = Instruction {
        program_id: argus::id(),
        accounts: argus::accounts::InvalidateCapability {
            authority: stranger.pubkey(),
            mint: w.mint,
            guard_config: Pubkey::find_program_address(&[b"guard", w.mint.as_ref()], &argus::id())
                .0,
            subject: bob.pubkey(),
            capability: w.capability_pda(bob.pubkey()),
        }
        .to_account_metas(None),
        data: argus::instruction::InvalidateCapability {}.data(),
    };
    assert!(
        w.send(&[bad], &[&stranger], &stranger.pubkey()).is_err(),
        "stranger invalidated a capability"
    );

    // Guard authority invalidates → capability zeroed → gate closes immediately,
    // even though the credential is still valid in aegis. (Fresh day dodges the
    // daily cap so only the capability gate can be responsible for the block.)
    w.warp_days(1);
    w.invalidate_capability(bob.pubkey()).unwrap();
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "gift passed on an invalidated capability"
    );
}

/// aegis stands on its own: authority gating, pause, and expiry validation.
#[test]
fn aegis_issuer_authority_pause_and_expiry() {
    let mut w = setup();
    let authority = Keypair::new();
    let stranger = Keypair::new();
    let subject = Keypair::new().pubkey();
    w.svm.airdrop(&authority.pubkey(), 5_000_000_000).unwrap();
    w.svm.airdrop(&stranger.pubkey(), 5_000_000_000).unwrap();

    let issuer = Pubkey::find_program_address(
        &[b"issuer", authority.pubkey().as_ref(), &0u64.to_le_bytes()],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::InitIssuer {
                authority: authority.pubkey(),
                issuer,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::InitIssuer {
                id: 0,
                name: "Oracle".into(),
            }
            .data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();

    let attestation = Pubkey::find_program_address(
        &[
            b"attestation",
            issuer.as_ref(),
            subject.as_ref(),
            &aegis::constants::well_known_schema::REGION.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;
    let issue_ix = |signer: Pubkey, expires_at: i64| Instruction {
        program_id: aegis::id(),
        accounts: aegis::accounts::IssueAttestation {
            signer,
            issuer,
            attestation,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: aegis::instruction::IssueAttestation {
            subject,
            data: aegis::instructions::attestation::AttestationData {
                schema_id: aegis::constants::well_known_schema::REGION,
                commitment: [1u8; 32],
                attr_root: [0u8; 32],
                valid_from: 0,
                expires_at,
            },
        }
        .data(),
    };

    // A stranger cannot issue on someone else's issuer.
    assert!(
        w.send(
            &[issue_ix(stranger.pubkey(), 0)],
            &[&stranger],
            &stranger.pubkey()
        )
        .is_err(),
        "stranger issued an attestation"
    );

    // Expiry in the past is rejected.
    let past = w.svm.get_sysvar::<Clock>().unix_timestamp - 10;
    assert!(
        w.send(
            &[issue_ix(authority.pubkey(), past)],
            &[&authority],
            &authority.pubkey()
        )
        .is_err(),
        "past expiry accepted"
    );

    // Valid issuance succeeds.
    w.send(
        &[issue_ix(authority.pubkey(), 0)],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();

    // Pausing the issuer blocks further issuance (update path).
    let subject2 = Keypair::new().pubkey();
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::IssuerAuthorityOnly {
                authority: authority.pubkey(),
                issuer,
            }
            .to_account_metas(None),
            data: aegis::instruction::SetIssuerPaused { paused: true }.data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    let attestation2 = Pubkey::find_program_address(
        &[
            b"attestation",
            issuer.as_ref(),
            subject2.as_ref(),
            &aegis::constants::well_known_schema::REGION.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;
    let issue2 = Instruction {
        program_id: aegis::id(),
        accounts: aegis::accounts::IssueAttestation {
            signer: authority.pubkey(),
            issuer,
            attestation: attestation2,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: aegis::instruction::IssueAttestation {
            subject: subject2,
            data: aegis::instructions::attestation::AttestationData {
                schema_id: aegis::constants::well_known_schema::REGION,
                commitment: [1u8; 32],
                attr_root: [0u8; 32],
                valid_from: 0,
                expires_at: 0,
            },
        }
        .data(),
    };
    assert!(
        w.send(&[issue2], &[&authority], &authority.pubkey())
            .is_err(),
        "paused issuer still issued"
    );
}

/// The hot/cold key split: an operator issues but cannot administer.
#[test]
fn aegis_operator_issues_but_cannot_administer() {
    let mut w = setup();
    let authority = Keypair::new();
    let operator = Keypair::new();
    let subject = Keypair::new().pubkey();
    w.svm.airdrop(&authority.pubkey(), 5_000_000_000).unwrap();
    w.svm.airdrop(&operator.pubkey(), 5_000_000_000).unwrap();

    let issuer = Pubkey::find_program_address(
        &[b"issuer", authority.pubkey().as_ref(), &0u64.to_le_bytes()],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::InitIssuer {
                authority: authority.pubkey(),
                issuer,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::InitIssuer {
                id: 0,
                name: "Oracle".into(),
            }
            .data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();

    // Grant the operator.
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::IssuerAuthorityOnly {
                authority: authority.pubkey(),
                issuer,
            }
            .to_account_metas(None),
            data: aegis::instruction::SetOperator {
                operator: Some(operator.pubkey()),
            }
            .data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();

    // Operator issues — allowed.
    let attestation = Pubkey::find_program_address(
        &[
            b"attestation",
            issuer.as_ref(),
            subject.as_ref(),
            &aegis::constants::well_known_schema::REGION.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::IssueAttestation {
                signer: operator.pubkey(),
                issuer,
                attestation,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::IssueAttestation {
                subject,
                data: aegis::instructions::attestation::AttestationData {
                    schema_id: aegis::constants::well_known_schema::REGION,
                    commitment: [1u8; 32],
                    attr_root: [0u8; 32],
                    valid_from: 0,
                    expires_at: 0,
                },
            }
            .data(),
        }],
        &[&operator],
        &operator.pubkey(),
    )
    .unwrap();

    // Operator tries to administer (pause) — rejected (authority only).
    assert!(
        w.send(
            &[Instruction {
                program_id: aegis::id(),
                accounts: aegis::accounts::IssuerAuthorityOnly {
                    authority: operator.pubkey(),
                    issuer,
                }
                .to_account_metas(None),
                data: aegis::instruction::SetIssuerPaused { paused: true }.data(),
            }],
            &[&operator],
            &operator.pubkey()
        )
        .is_err(),
        "operator administered the issuer"
    );
}

/// Closing an attestation returns its rent to the issuer authority.
#[test]
fn aegis_close_reclaims_rent() {
    let mut w = setup();
    let authority = Keypair::new();
    let subject = Keypair::new().pubkey();
    w.svm.airdrop(&authority.pubkey(), 5_000_000_000).unwrap();

    let issuer = Pubkey::find_program_address(
        &[b"issuer", authority.pubkey().as_ref(), &0u64.to_le_bytes()],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::InitIssuer {
                authority: authority.pubkey(),
                issuer,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::InitIssuer {
                id: 0,
                name: "Oracle".into(),
            }
            .data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();

    let attestation = Pubkey::find_program_address(
        &[
            b"attestation",
            issuer.as_ref(),
            subject.as_ref(),
            &aegis::constants::well_known_schema::REGION.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::IssueAttestation {
                signer: authority.pubkey(),
                issuer,
                attestation,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::IssueAttestation {
                subject,
                data: aegis::instructions::attestation::AttestationData {
                    schema_id: aegis::constants::well_known_schema::REGION,
                    commitment: [1u8; 32],
                    attr_root: [0u8; 32],
                    valid_from: 0,
                    expires_at: 0,
                },
            }
            .data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    assert!(w
        .svm
        .get_account(&attestation)
        .is_some_and(|a| !a.data.is_empty()));

    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::CloseAttestation {
                signer: authority.pubkey(),
                issuer,
                authority: authority.pubkey(),
                attestation,
            }
            .to_account_metas(None),
            data: aegis::instruction::CloseAttestation {}.data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    // Closed → account gone (or zero-lamport, empty).
    assert!(
        w.svm
            .get_account(&attestation)
            .is_none_or(|a| a.lamports == 0),
        "attestation not closed"
    );
}

/// argus honors a not-before window: pre-issued attestations gate until valid.
#[test]
fn aegis_valid_from_gates_until_active() {
    let issuer_authority = Keypair::new();
    let issuer = Pubkey::find_program_address(
        &[
            b"issuer",
            issuer_authority.pubkey().as_ref(),
            &0u64.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;

    let mut policy = base_policy();
    policy.flags |= argus::constants::flags::REQUIRE_ATTESTATION;
    policy.aegis_program = aegis::id();
    policy.attestation_issuer = issuer;
    policy.attestation_schema = aegis::constants::well_known_schema::REGION;

    let mut w = setup_with_policy(policy);
    w.svm
        .airdrop(&issuer_authority.pubkey(), 5_000_000_000)
        .unwrap();
    let alice = Keypair::new();
    let bob = Keypair::new();
    let schema_id = aegis::constants::well_known_schema::REGION;
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);

    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::InitIssuer {
                authority: issuer_authority.pubkey(),
                issuer,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::InitIssuer {
                id: 0,
                name: "Geo".into(),
            }
            .data(),
        }],
        &[&issuer_authority],
        &issuer_authority.pubkey(),
    )
    .unwrap();

    // Pre-issue with valid_from one day out.
    let now = w.svm.get_sysvar::<Clock>().unix_timestamp;
    let attestation = Pubkey::find_program_address(
        &[
            b"attestation",
            issuer.as_ref(),
            bob.pubkey().as_ref(),
            &schema_id.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::IssueAttestation {
                signer: issuer_authority.pubkey(),
                issuer,
                attestation,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::IssueAttestation {
                subject: bob.pubkey(),
                data: aegis::instructions::attestation::AttestationData {
                    schema_id,
                    commitment: [1u8; 32],
                    attr_root: [0u8; 32],
                    valid_from: now + 86_400,
                    expires_at: 0,
                },
            }
            .data(),
        }],
        &[&issuer_authority],
        &issuer_authority.pubkey(),
    )
    .unwrap();

    // Not yet valid → `verify` returns out-of-window → capability bit unset → rejected.
    w.refresh_eligibility(&alice, bob.pubkey(), issuer, schema_id)
        .unwrap();
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "pre-active attestation passed"
    );

    // After the window opens → refresh flips positive → allowed.
    w.warp_days(1);
    w.refresh_eligibility(&alice, bob.pubkey(), issuer, schema_id)
        .unwrap();
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.state_of(alice.pubkey()).sent_today, 1_000);
}

/// Revocation is terminal: an update cannot silently reinstate a revoked credential.
#[test]
fn aegis_revocation_is_sticky() {
    let mut w = setup();
    let authority = Keypair::new();
    let subject = Keypair::new().pubkey();
    w.svm.airdrop(&authority.pubkey(), 5_000_000_000).unwrap();
    let issuer = Pubkey::find_program_address(
        &[b"issuer", authority.pubkey().as_ref(), &0u64.to_le_bytes()],
        &aegis::id(),
    )
    .0;
    let attestation = Pubkey::find_program_address(
        &[
            b"attestation",
            issuer.as_ref(),
            subject.as_ref(),
            &aegis::constants::well_known_schema::REGION.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::InitIssuer {
                authority: authority.pubkey(),
                issuer,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::InitIssuer {
                id: 0,
                name: "Oracle".into(),
            }
            .data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    let data = aegis::instructions::attestation::AttestationData {
        schema_id: aegis::constants::well_known_schema::REGION,
        commitment: [1u8; 32],
        attr_root: [0u8; 32],
        valid_from: 0,
        expires_at: 0,
    };
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::IssueAttestation {
                signer: authority.pubkey(),
                issuer,
                attestation,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::IssueAttestation {
                subject,
                data: data.clone(),
            }
            .data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    // Revoke.
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::ManageAttestation {
                signer: authority.pubkey(),
                issuer,
                attestation,
            }
            .to_account_metas(None),
            data: aegis::instruction::RevokeAttestation { reason_code: 1 }.data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    // An update must NOT un-revoke it.
    assert!(
        w.send(
            &[Instruction {
                program_id: aegis::id(),
                accounts: aegis::accounts::ManageAttestation {
                    signer: authority.pubkey(),
                    issuer,
                    attestation,
                }
                .to_account_metas(None),
                data: aegis::instruction::UpdateAttestation { data }.data(),
            }],
            &[&authority],
            &authority.pubkey()
        )
        .is_err(),
        "update reinstated a revoked attestation"
    );
}

#[test]
fn argus_hardcoded_constants_match_dependencies() {
    use anchor_lang::Discriminator;
    assert_eq!(argus::constants::VESTA_CORE_ID, vesta_core::id());
    assert_eq!(
        argus::constants::MERCHANT_DISCRIMINATOR,
        vesta_core::state::Merchant::DISCRIMINATOR
    );
    assert_eq!(argus::constants::AEGIS_ID, aegis::id());
}

/// The aegis policy engine (spec 07 phase 2): a named, jurisdiction-tagged,
/// freshness-checked verdict a verifier resolves by policy id.
#[test]
fn aegis_policy_engine_returns_named_verdicts() {
    let mut w = setup();
    let authority = Keypair::new();
    let subject = Keypair::new().pubkey();
    w.svm.airdrop(&authority.pubkey(), 5_000_000_000).unwrap();
    let schema_id = aegis::constants::well_known_schema::KYC_TIER;

    let issuer = Pubkey::find_program_address(
        &[b"issuer", authority.pubkey().as_ref(), &0u64.to_le_bytes()],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::InitIssuer {
                authority: authority.pubkey(),
                issuer,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::InitIssuer {
                id: 0,
                name: "KYC".into(),
            }
            .data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();

    let attestation = Pubkey::find_program_address(
        &[
            b"attestation",
            issuer.as_ref(),
            subject.as_ref(),
            &schema_id.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::IssueAttestation {
                signer: authority.pubkey(),
                issuer,
                attestation,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::IssueAttestation {
                subject,
                data: aegis::instructions::attestation::AttestationData {
                    schema_id,
                    commitment: [9u8; 32],
                    attr_root: [0u8; 32],
                    valid_from: 0,
                    expires_at: 0,
                },
            }
            .data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();

    // Register two policies: one matching, one requiring a fresh (≤1h) credential.
    let register = |id: u64, freshness: i64| Instruction {
        program_id: aegis::id(),
        accounts: aegis::accounts::RegisterPolicy {
            authority: authority.pubkey(),
            policy: Pubkey::find_program_address(
                &[b"policy", authority.pubkey().as_ref(), &id.to_le_bytes()],
                &aegis::id(),
            )
            .0,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: aegis::instruction::RegisterPolicy {
            id,
            jurisdiction: 1,
            issuer,
            schema_id,
            freshness_secs: freshness,
        }
        .data(),
    };
    w.send(&[register(0, 0)], &[&authority], &authority.pubkey())
        .unwrap();
    w.send(&[register(1, 3_600)], &[&authority], &authority.pubkey())
        .unwrap();

    let policy_pda = |id: u64| {
        Pubkey::find_program_address(
            &[b"policy", authority.pubkey().as_ref(), &id.to_le_bytes()],
            &aegis::id(),
        )
        .0
    };

    // verify_policy never reverts — it returns a Verdict via return-data.
    let verdict = |w: &mut World, policy_id: u64| -> aegis::Verdict {
        let ix = Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::VerifyPolicy {
                policy: policy_pda(policy_id),
                attestation,
            }
            .to_account_metas(None),
            data: aegis::instruction::VerifyPolicy { subject }.data(),
        };
        let mut msg = Message::new(&[ix], Some(&authority.pubkey()));
        msg.recent_blockhash = w.svm.latest_blockhash();
        let tx =
            VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&authority]).unwrap();
        let meta = w.svm.send_transaction(tx).unwrap();
        w.svm.expire_blockhash();
        aegis::Verdict::try_from_slice(&meta.return_data.data).unwrap()
    };

    // Matching policy → ok.
    assert!(
        verdict(&mut w, 0).ok,
        "matching policy rejected a valid credential"
    );
    // Fresh-required policy, credential just issued → still ok.
    assert!(
        verdict(&mut w, 1).ok,
        "fresh policy rejected a just-issued credential"
    );
    // Warp past the freshness window → the fresh policy now fails (TOO_OLD),
    // while the no-freshness policy still passes.
    w.warp_days(1);
    assert!(
        verdict(&mut w, 0).ok,
        "no-freshness policy failed after time passed"
    );
    assert!(
        !verdict(&mut w, 1).ok,
        "stale credential passed a fresh-required policy"
    );

    // Revoke → both policies fail.
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::ManageAttestation {
                signer: authority.pubkey(),
                issuer,
                attestation,
            }
            .to_account_metas(None),
            data: aegis::instruction::RevokeAttestation { reason_code: 1 }.data(),
        }],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    assert!(
        !verdict(&mut w, 0).ok,
        "revoked credential passed the policy"
    );
}

/// Track B x C: argus enforces an aegis *Policy* (spec 07/09). The compliance
/// rule (jurisdiction / schema / freshness) lives in aegis as data — argus just
/// caches the policy verdict. Changing the rule needs NO argus redeploy.
#[test]
fn argus_gates_on_an_aegis_policy() {
    let issuer_authority = Keypair::new();
    let policy_authority = Keypair::new();
    let issuer = Pubkey::find_program_address(
        &[
            b"issuer",
            issuer_authority.pubkey().as_ref(),
            &0u64.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;
    let aegis_policy = Pubkey::find_program_address(
        &[
            b"policy",
            policy_authority.pubkey().as_ref(),
            &0u64.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;
    let schema_id = aegis::constants::well_known_schema::KYC_TIER;

    let mut policy = base_policy();
    policy.flags |= argus::constants::flags::REQUIRE_ATTESTATION;
    policy.aegis_program = aegis::id();
    policy.policy = aegis_policy; // enforce the aegis Policy, not a raw Present
    let mut w = setup_with_policy(policy);
    w.svm
        .airdrop(&issuer_authority.pubkey(), 5_000_000_000)
        .unwrap();
    w.svm
        .airdrop(&policy_authority.pubkey(), 5_000_000_000)
        .unwrap();

    let alice = Keypair::new();
    let bob = Keypair::new();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);

    // aegis: issuer, a jurisdiction-tagged KYC policy, and bob's credential.
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::InitIssuer {
                authority: issuer_authority.pubkey(),
                issuer,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::InitIssuer {
                id: 0,
                name: "KYC".into(),
            }
            .data(),
        }],
        &[&issuer_authority],
        &issuer_authority.pubkey(),
    )
    .unwrap();
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::RegisterPolicy {
                authority: policy_authority.pubkey(),
                policy: aegis_policy,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::RegisterPolicy {
                id: 0,
                jurisdiction: 42,
                issuer,
                schema_id,
                freshness_secs: 0,
            }
            .data(),
        }],
        &[&policy_authority],
        &policy_authority.pubkey(),
    )
    .unwrap();

    let attestation = Pubkey::find_program_address(
        &[
            b"attestation",
            issuer.as_ref(),
            bob.pubkey().as_ref(),
            &schema_id.to_le_bytes(),
        ],
        &aegis::id(),
    )
    .0;

    // Refresh against the aegis Policy (argus CPIs verify_policy).
    let refresh = |w: &World| Instruction {
        program_id: argus::id(),
        accounts: argus::accounts::RefreshEligibility {
            payer: alice.pubkey(),
            mint: w.mint,
            guard_config: w.guard_config(),
            subject: bob.pubkey(),
            capability: Pubkey::find_program_address(
                &[b"cap", w.mint.as_ref(), bob.pubkey().as_ref()],
                &argus::id(),
            )
            .0,
            attestation,
            policy: aegis_policy,
            aegis_program: aegis::id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: argus::instruction::RefreshEligibility {}.data(),
    };

    // No credential yet → policy verdict negative → transfer rejected.
    let r = refresh(&w);
    w.send(&[r], &[&alice], &alice.pubkey()).unwrap();
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "policy-gated transfer passed without a credential"
    );

    // Issue bob the KYC credential + refresh → policy passes → transfer allowed.
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::IssueAttestation {
                signer: issuer_authority.pubkey(),
                issuer,
                attestation,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::IssueAttestation {
                subject: bob.pubkey(),
                data: aegis::instructions::attestation::AttestationData {
                    schema_id,
                    commitment: [5u8; 32],
                    attr_root: [0u8; 32],
                    valid_from: 0,
                    expires_at: 0,
                },
            }
            .data(),
        }],
        &[&issuer_authority],
        &issuer_authority.pubkey(),
    )
    .unwrap();
    let r = refresh(&w);
    w.send(&[r], &[&alice], &alice.pubkey()).unwrap();
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        issuer,
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.state_of(alice.pubkey()).sent_today, 1_000);
}

/// aegis 08: the issuer accreditation trust graph. A verifier pins one root;
/// issuers it accredits inherit trust. verify_accreditation is a return-data
/// verdict, composable with a credential verify.
#[test]
fn aegis_accreditation_trust_graph() {
    let mut w = setup();
    let root_authority = Keypair::new();
    let subject_issuer = Keypair::new().pubkey(); // the accredited issuer's PDA (opaque here)
    let other_issuer = Keypair::new().pubkey();
    w.svm
        .airdrop(&root_authority.pubkey(), 5_000_000_000)
        .unwrap();
    let kyc = aegis::constants::well_known_schema::KYC_TIER;
    let region = aegis::constants::well_known_schema::REGION;

    let trust_root =
        Pubkey::find_program_address(&[b"troot", root_authority.pubkey().as_ref()], &aegis::id()).0;
    let accred = |issuer: Pubkey| {
        Pubkey::find_program_address(
            &[b"accred", root_authority.pubkey().as_ref(), issuer.as_ref()],
            &aegis::id(),
        )
        .0
    };

    // Declare the root and accredit `subject_issuer` for KYC only.
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::RegisterTrustRoot {
                authority: root_authority.pubkey(),
                trust_root,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::RegisterTrustRoot {
                name: "EU-Compliance".into(),
            }
            .data(),
        }],
        &[&root_authority],
        &root_authority.pubkey(),
    )
    .unwrap();
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::AccreditIssuer {
                authority: root_authority.pubkey(),
                trust_root,
                accreditation: accred(subject_issuer),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::AccreditIssuer {
                subject_issuer,
                tier: 2,
                permitted_schemas: vec![kyc],
                jurisdiction: 42,
                expires_at: 0,
            }
            .data(),
        }],
        &[&root_authority],
        &root_authority.pubkey(),
    )
    .unwrap();

    let verdict = |w: &mut World, issuer: Pubkey, schema: u64| -> aegis::Verdict {
        let ix = Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::VerifyAccreditation {
                trust_root,
                accreditation: accred(issuer),
            }
            .to_account_metas(None),
            data: aegis::instruction::VerifyAccreditation {
                root: root_authority.pubkey(),
                subject_issuer: issuer,
                schema_id: schema,
            }
            .data(),
        };
        let mut msg = Message::new(&[ix], Some(&root_authority.pubkey()));
        msg.recent_blockhash = w.svm.latest_blockhash();
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&root_authority])
            .unwrap();
        let meta = w.svm.send_transaction(tx).unwrap();
        w.svm.expire_blockhash();
        aegis::Verdict::try_from_slice(&meta.return_data.data).unwrap()
    };

    // Accredited issuer, permitted schema → ok.
    assert!(
        verdict(&mut w, subject_issuer, kyc).ok,
        "accredited KYC rejected"
    );
    // Same issuer, non-permitted schema → rejected.
    assert!(
        !verdict(&mut w, subject_issuer, region).ok,
        "non-permitted schema accepted"
    );
    // A different, unaccredited issuer → rejected.
    assert!(
        !verdict(&mut w, other_issuer, kyc).ok,
        "unaccredited issuer accepted"
    );

    // Revoke → the whole issuer de-trusts instantly.
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::RevokeAccreditation {
                authority: root_authority.pubkey(),
                trust_root,
                accreditation: accred(subject_issuer),
            }
            .to_account_metas(None),
            data: aegis::instruction::RevokeAccreditation {}.data(),
        }],
        &[&root_authority],
        &root_authority.pubkey(),
    )
    .unwrap();
    assert!(
        !verdict(&mut w, subject_issuer, kyc).ok,
        "revoked accreditation still trusted"
    );
}

// ── Governance (spec 10, phase 1) ────────────────────────────────────────────

use argus::instructions::governance::RoleAssignment;
use argus::state::PolicyDoc;

/// Build a role assignment with distinct keys (SoD-friendly), all sharing
/// `admin` as RoleAdmin. Returns the assignment plus the freshly-funded keys.
fn gov_roles(w: &mut World) -> (RoleAssignment, Keypair, Keypair, Keypair, Keypair) {
    let role_admin = Keypair::new();
    let author = Keypair::new();
    let approver = Keypair::new();
    let activator = Keypair::new();
    for k in [&role_admin, &author, &approver, &activator] {
        w.svm.airdrop(&k.pubkey(), 5_000_000_000).unwrap();
    }
    let assignment = RoleAssignment {
        role_admin: role_admin.pubkey(),
        author: author.pubkey(),
        approver: approver.pubkey(),
        activator: activator.pubkey(),
        pause_operator: role_admin.pubkey(),
        reporter: role_admin.pubkey(),
    };
    (assignment, role_admin, author, approver, activator)
}

/// A doc that differs from `base_policy` so activation is observable.
fn amended_doc() -> PolicyDoc {
    PolicyDoc {
        flags: argus::constants::flags::BLOCK_PROGRAM_OWNED,
        daily_gift_cap: CAP,
        per_tx_cap: 111,
        max_wallet_balance: 0,
        transfers_per_day_cap: 0,
        cooldown_secs: 0,
        attestation_schema: 0,
        capability_ttl_secs: 0,
        policy: Pubkey::default(),
    }
}

#[test]
fn argus_governance_full_lifecycle() {
    let mut w = setup_with_policy(base_policy());
    let (roles, _admin, author, approver, activator) = gov_roles(&mut w);

    w.init_governance(roles, 3_600).unwrap();
    assert!(w.config_of().governed, "governance flag not set");
    let epoch0 = w.config_of().policy_epoch;

    // Free-tier live mutation is now disabled.
    let merchant = w.merchant_authority.insecure_clone();
    let bad = Instruction {
        program_id: argus::id(),
        accounts: argus::accounts::GuardAuthorityOnly {
            authority: merchant.pubkey(),
            guard_config: w.guard_config(),
        }
        .to_account_metas(None),
        data: argus::instruction::ConfigurePolicy {
            update: argus::instructions::policy::PolicyUpdate::default(),
        }
        .data(),
    };
    assert!(
        w.send(&[bad], &[&merchant], &merchant.pubkey()).is_err(),
        "configure_policy worked on a governed mint"
    );

    // propose → approve → timelock → activate.
    let doc = amended_doc();
    let hash = doc.hash().unwrap();
    w.propose_policy(&author, doc).unwrap();
    w.approve_policy(&approver, hash).unwrap();

    // Too early — timelock still running.
    assert!(
        w.activate_or_rollback(&activator, hash, false).is_err(),
        "activated before timelock elapsed"
    );

    w.warp_secs(3_601);
    w.activate_or_rollback(&activator, hash, false).unwrap();

    let cfg = w.config_of();
    assert_eq!(cfg.per_tx_cap, 111, "activated doc not applied");
    assert_eq!(cfg.policy_epoch, epoch0 + 1, "epoch not bumped");
    assert_eq!(w.pointer_of().active_hash, hash);
    assert_eq!(w.pointer_of().pending_hash, [0u8; 32]);
}

#[test]
fn argus_governance_self_approval_rejected() {
    let mut w = setup_with_policy(base_policy());
    let (mut roles, _admin, author, _approver, _activator) = gov_roles(&mut w);
    // Make the author also the approver → activation becomes impossible.
    roles.approver = author.pubkey();
    w.init_governance(roles, 0).unwrap();

    let doc = amended_doc();
    let hash = doc.hash().unwrap();
    w.propose_policy(&author, doc).unwrap();
    assert!(
        w.approve_policy(&author, hash).is_err(),
        "author approved their own proposal"
    );
}

#[test]
fn argus_governance_role_not_held_rejected() {
    let mut w = setup_with_policy(base_policy());
    let (roles, _admin, _author, approver, _activator) = gov_roles(&mut w);
    w.init_governance(roles, 0).unwrap();

    // `approver` is not the Author → cannot propose.
    let doc = amended_doc();
    assert!(
        w.propose_policy(&approver, doc).is_err(),
        "non-author proposed a policy"
    );
}

#[test]
fn argus_governance_pin_blocks_further_change() {
    let mut w = setup_with_policy(base_policy());
    let (roles, admin, author, _approver, _activator) = gov_roles(&mut w);
    w.init_governance(roles, 0).unwrap();

    // Pin the active (genesis) version.
    let pin = Instruction {
        program_id: argus::id(),
        accounts: argus::accounts::PinPolicy {
            role_admin: admin.pubkey(),
            role_registry: w.roles_pda(),
            policy_pointer: w.pointer_pda(),
        }
        .to_account_metas(None),
        data: argus::instruction::PinPolicy {}.data(),
    };
    w.send(&[pin], &[&admin], &admin.pubkey()).unwrap();
    assert!(w.pointer_of().pinned);

    // No further proposals accepted.
    assert!(
        w.propose_policy(&author, amended_doc()).is_err(),
        "proposed a policy on a pinned mint"
    );
}

#[test]
fn argus_governance_rollback_restores_prior_version() {
    let mut w = setup_with_policy(base_policy());
    let (roles, _admin, author, approver, activator) = gov_roles(&mut w);
    w.init_governance(roles, 0).unwrap();
    let genesis_hash = w.pointer_of().active_hash;

    // Activate an amended version.
    let doc = amended_doc();
    let hash = doc.hash().unwrap();
    w.propose_policy(&author, doc).unwrap();
    w.approve_policy(&approver, hash).unwrap();
    w.activate_or_rollback(&activator, hash, false).unwrap();
    assert_eq!(w.config_of().per_tx_cap, 111);

    // Expedited rollback to the (approved) genesis version.
    w.activate_or_rollback(&activator, genesis_hash, true)
        .unwrap();
    assert_eq!(w.config_of().per_tx_cap, 0, "rollback did not restore");
    assert_eq!(w.pointer_of().active_hash, genesis_hash);
}

#[test]
fn argus_governance_role_change_is_timelocked() {
    let mut w = setup_with_policy(base_policy());
    let (roles, admin, _author, _approver, _activator) = gov_roles(&mut w);
    w.init_governance(roles, 3_600).unwrap();

    let new_author = Keypair::new();
    let propose = |w: &World| Instruction {
        program_id: argus::id(),
        accounts: argus::accounts::RoleChange {
            role_admin: admin.pubkey(),
            role_registry: w.roles_pda(),
            policy_pointer: w.pointer_pda(),
        }
        .to_account_metas(None),
        data: argus::instruction::ProposeRoleChange {
            role: 1, // Author
            authority: new_author.pubkey(),
        }
        .data(),
    };
    let ix = propose(&w);
    w.send(&[ix], &[&admin], &admin.pubkey()).unwrap();

    let apply = |w: &World| Instruction {
        program_id: argus::id(),
        accounts: argus::accounts::RoleChange {
            role_admin: admin.pubkey(),
            role_registry: w.roles_pda(),
            policy_pointer: w.pointer_pda(),
        }
        .to_account_metas(None),
        data: argus::instruction::ApplyRoleChange {}.data(),
    };

    // Too early.
    let early = apply(&w);
    assert!(
        w.send(&[early], &[&admin], &admin.pubkey()).is_err(),
        "role change applied before timelock"
    );

    w.warp_secs(3_601);
    let ok = apply(&w);
    w.send(&[ok], &[&admin], &admin.pubkey()).unwrap();
    assert_eq!(
        w.roles_of().author,
        new_author.pubkey(),
        "author not rotated"
    );

    // A stranger cannot drive role changes.
    let stranger = Keypair::new();
    w.svm.airdrop(&stranger.pubkey(), 5_000_000_000).unwrap();
    let bad = Instruction {
        program_id: argus::id(),
        accounts: argus::accounts::RoleChange {
            role_admin: stranger.pubkey(),
            role_registry: w.roles_pda(),
            policy_pointer: w.pointer_pda(),
        }
        .to_account_metas(None),
        data: argus::instruction::ProposeRoleChange {
            role: 1,
            authority: stranger.pubkey(),
        }
        .data(),
    };
    assert!(
        w.send(&[bad], &[&stranger], &stranger.pubkey()).is_err(),
        "non-admin drove a role change"
    );
}

#[test]
fn argus_governance_anchor_statement() {
    let mut w = setup_with_policy(base_policy());
    // gov_roles sets reporter = role_admin.
    let (roles, reporter, _author, _approver, _activator) = gov_roles(&mut w);
    w.init_governance(roles, 0).unwrap();

    let root = [9u8; 32];
    w.anchor_statement(&reporter, 20_240, root, 42).unwrap();
    let s = w.statement_of(20_240);
    assert_eq!(s.merkle_root, root);
    assert_eq!(s.decision_count, 42);
    assert_eq!(s.period, 20_240);
    assert_eq!(s.reporter, reporter.pubkey());

    // A period is anchored once (immutable, append-only).
    assert!(
        w.anchor_statement(&reporter, 20_240, [1u8; 32], 1).is_err(),
        "re-anchored an existing period"
    );

    // Only the Reporter role may anchor.
    let stranger = Keypair::new();
    w.svm.airdrop(&stranger.pubkey(), 5_000_000_000).unwrap();
    assert!(
        w.anchor_statement(&stranger, 20_241, root, 1).is_err(),
        "non-reporter anchored a statement"
    );
}

// ── Trust triangle (spec 10, phase 3) ────────────────────────────────────────

/// Register an aegis trust root and accredit `subject_issuer` for `schema`.
fn aegis_setup_accreditation(
    w: &mut World,
    root_authority: &Keypair,
    subject_issuer: Pubkey,
    schema: u64,
) {
    w.svm
        .airdrop(&root_authority.pubkey(), 5_000_000_000)
        .unwrap();
    let trust_root =
        Pubkey::find_program_address(&[b"troot", root_authority.pubkey().as_ref()], &aegis::id()).0;
    let accred = Pubkey::find_program_address(
        &[
            b"accred",
            root_authority.pubkey().as_ref(),
            subject_issuer.as_ref(),
        ],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::RegisterTrustRoot {
                authority: root_authority.pubkey(),
                trust_root,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::RegisterTrustRoot { name: "Reg".into() }.data(),
        }],
        &[root_authority],
        &root_authority.pubkey(),
    )
    .unwrap();
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::AccreditIssuer {
                authority: root_authority.pubkey(),
                trust_root,
                accreditation: accred,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: aegis::instruction::AccreditIssuer {
                subject_issuer,
                tier: 2,
                permitted_schemas: vec![schema],
                jurisdiction: 42,
                expires_at: 0,
            }
            .data(),
        }],
        &[root_authority],
        &root_authority.pubkey(),
    )
    .unwrap();
}

fn aegis_revoke_accreditation(w: &mut World, root_authority: &Keypair, subject_issuer: Pubkey) {
    let trust_root =
        Pubkey::find_program_address(&[b"troot", root_authority.pubkey().as_ref()], &aegis::id()).0;
    let accred = Pubkey::find_program_address(
        &[
            b"accred",
            root_authority.pubkey().as_ref(),
            subject_issuer.as_ref(),
        ],
        &aegis::id(),
    )
    .0;
    w.send(
        &[Instruction {
            program_id: aegis::id(),
            accounts: aegis::accounts::RevokeAccreditation {
                authority: root_authority.pubkey(),
                trust_root,
                accreditation: accred,
            }
            .to_account_metas(None),
            data: aegis::instruction::RevokeAccreditation {}.data(),
        }],
        &[root_authority],
        &root_authority.pubkey(),
    )
    .unwrap();
}

#[test]
fn argus_trust_triangle_auto_degrade_and_restore() {
    let mut policy = base_policy();
    policy.aegis_program = aegis::id();
    let mut w = setup_with_policy(policy);
    let root_authority = Keypair::new();
    let subject_issuer = Keypair::new().pubkey();
    let schema = aegis::constants::well_known_schema::KYC_TIER;
    aegis_setup_accreditation(&mut w, &root_authority, subject_issuer, schema);

    w.set_trust_anchor(
        root_authority.pubkey(),
        subject_issuer,
        schema,
        argus::constants::degrade::FROZEN,
        0,
    )
    .unwrap();

    let cranker = Keypair::new();
    w.svm.airdrop(&cranker.pubkey(), 5_000_000_000).unwrap();

    // Healthy accreditation → posture NORMAL.
    w.reverify(&cranker, root_authority.pubkey(), subject_issuer)
        .unwrap();
    assert_eq!(
        w.config_of().degrade_mode,
        argus::constants::degrade::NORMAL
    );

    // A peer gift flows.
    let alice = Keypair::new();
    let bob = Keypair::new();
    prime_sender(&mut w, &alice, bob.pubkey(), 5_000);
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        Pubkey::default(),
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();

    // Revoke accreditation → crank trips the posture (grace = 0).
    aegis_revoke_accreditation(&mut w, &root_authority, subject_issuer);
    w.reverify(&cranker, root_authority.pubkey(), subject_issuer)
        .unwrap();
    assert_eq!(
        w.config_of().degrade_mode,
        argus::constants::degrade::FROZEN
    );

    // Peer gift now blocked.
    w.warp_days(1);
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        Pubkey::default(),
        true,
    );
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "peer gift passed under a degraded posture"
    );

    // Manual restore (challenge path) reopens peer transfers.
    w.set_degrade(argus::constants::degrade::NORMAL).unwrap();
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        1_000,
        Pubkey::default(),
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
}

#[test]
fn argus_trust_grace_window_absorbs_transient_failure() {
    let mut policy = base_policy();
    policy.aegis_program = aegis::id();
    let mut w = setup_with_policy(policy);
    let root_authority = Keypair::new();
    let subject_issuer = Keypair::new().pubkey();
    let schema = aegis::constants::well_known_schema::KYC_TIER;
    aegis_setup_accreditation(&mut w, &root_authority, subject_issuer, schema);

    w.set_trust_anchor(
        root_authority.pubkey(),
        subject_issuer,
        schema,
        argus::constants::degrade::REDEMPTION_ONLY,
        3_600,
    )
    .unwrap();
    let cranker = Keypair::new();
    w.svm.airdrop(&cranker.pubkey(), 5_000_000_000).unwrap();
    w.reverify(&cranker, root_authority.pubkey(), subject_issuer)
        .unwrap();

    // Revoke, then crank inside the grace window → still NORMAL.
    aegis_revoke_accreditation(&mut w, &root_authority, subject_issuer);
    w.reverify(&cranker, root_authority.pubkey(), subject_issuer)
        .unwrap();
    assert_eq!(
        w.config_of().degrade_mode,
        argus::constants::degrade::NORMAL,
        "degraded before the grace window elapsed"
    );

    // Past the grace window → degrade bites.
    w.warp_secs(3_601);
    w.reverify(&cranker, root_authority.pubkey(), subject_issuer)
        .unwrap();
    assert_eq!(
        w.config_of().degrade_mode,
        argus::constants::degrade::REDEMPTION_ONLY
    );
}
