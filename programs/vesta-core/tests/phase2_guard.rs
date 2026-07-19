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
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::associated_token::get_associated_token_address_with_program_id,
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
const CAP: u64 = argus::constants::DAILY_GIFT_CAP_RAW;

fn core_bytes() -> &'static [u8] {
    include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/vesta_core.so"
    ))
}
fn argus_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/argus.so"))
}

struct World {
    svm: LiteSVM,
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
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(Box::new)?;
        self.svm.expire_blockhash();
        Ok(())
    }

    fn warp_days(&mut self, days: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp += days * 86_400;
        self.svm.set_sysvar::<Clock>(&clock);
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

    fn guard_init_ix(&self, signer: Pubkey, merchant: Pubkey, mint: Pubkey) -> Instruction {
        Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::InitializeTransferGuard {
                merchant_authority: signer,
                merchant,
                mint,
                extra_account_meta_list: Pubkey::find_program_address(
                    &[b"extra-account-metas", mint.as_ref()],
                    &argus::id(),
                )
                .0,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: argus::instruction::InitializeTransferGuard {}.data(),
        }
    }

    fn open_ledger(&mut self, owner: &Keypair) -> Pubkey {
        let ledger = Pubkey::find_program_address(
            &[b"ledger", self.mint.as_ref(), owner.pubkey().as_ref()],
            &argus::id(),
        )
        .0;
        let ix = Instruction {
            program_id: argus::id(),
            accounts: argus::accounts::OpenGiftLedger {
                owner: owner.pubkey(),
                mint: self.mint,
                gift_ledger: ledger,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: argus::instruction::OpenGiftLedger {}.data(),
        };
        self.send(&[ix], &[owner], &owner.pubkey()).unwrap();
        ledger
    }

    /// transfer_checked with the hook extras appended in meta-list order.
    fn hooked_transfer_ix(
        &self,
        source_owner: Pubkey,
        authority: Pubkey,
        destination_wallet: Pubkey,
        amount: u64,
        with_extras: bool,
    ) -> Instruction {
        let source =
            get_associated_token_address_with_program_id(&source_owner, &self.mint, &TOKEN_2022_ID);
        let destination = get_associated_token_address_with_program_id(
            &destination_wallet,
            &self.mint,
            &TOKEN_2022_ID,
        );
        let ledger = Pubkey::find_program_address(
            &[b"ledger", self.mint.as_ref(), source_owner.as_ref()],
            &argus::id(),
        )
        .0;
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
            ix.accounts.push(AccountMeta::new(ledger, false));
            ix.accounts
                .push(AccountMeta::new_readonly(destination_wallet, false));
            ix.accounts
                .push(AccountMeta::new_readonly(self.treasury, false));
            ix.accounts
                .push(AccountMeta::new_readonly(argus::id(), false));
            ix.accounts
                .push(AccountMeta::new_readonly(self.eaml, false));
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

    fn ledger_state(&self, owner: Pubkey) -> argus::state::GiftLedger {
        let ledger = Pubkey::find_program_address(
            &[b"ledger", self.mint.as_ref(), owner.as_ref()],
            &argus::id(),
        )
        .0;
        let data = self.svm.get_account(&ledger).unwrap().data;
        argus::state::GiftLedger::try_deserialize(&mut data.as_slice()).unwrap()
    }
}

fn setup() -> World {
    let mut svm = LiteSVM::new();
    svm.add_program(vesta_core::id(), core_bytes()).unwrap();
    svm.add_program(argus::id(), argus_bytes()).unwrap();
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
        &[MERCHANT_SEED, merchant_authority.pubkey().as_ref()],
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

fn setup_with_guard() -> World {
    let mut w = setup();
    let authority = w.merchant_authority.insecure_clone();
    let ix = w.guard_init_ix(authority.pubkey(), w.merchant, w.mint);
    w.send(&[ix], &[&authority], &authority.pubkey()).unwrap();
    w
}

#[test]
fn guard_init_requires_the_merchant_authority() {
    let mut w = setup();
    let rando = Keypair::new();
    w.svm.airdrop(&rando.pubkey(), 5_000_000_000).unwrap();
    let ix = w.guard_init_ix(rando.pubkey(), w.merchant, w.mint);
    assert!(
        w.send(&[ix], &[&rando], &rando.pubkey()).is_err(),
        "front-runner initialized the guard"
    );
}

#[test]
fn guard_init_survives_prefund_griefing_and_rejects_double_init() {
    let mut w = setup();
    w.svm.airdrop(&w.eaml, 1).unwrap();

    let authority = w.merchant_authority.insecure_clone();
    let ix = w.guard_init_ix(authority.pubkey(), w.merchant, w.mint);
    w.send(
        std::slice::from_ref(&ix),
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    assert_eq!(w.svm.get_account(&w.eaml).unwrap().owner, argus::id());

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

    // Before guard init → rejected.
    assert!(w
        .send(
            std::slice::from_ref(&finalize),
            &[&authority],
            &authority.pubkey()
        )
        .is_err());

    let init = w.guard_init_ix(authority.pubkey(), w.merchant, w.mint);
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

    // Second finalize → GuardAlreadyFinalized.
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
    w.svm.airdrop(&alice.pubkey(), 10_000_000_000).unwrap();
    w.earn(alice.pubkey(), 5_000); // 510_000 raw at 1.02x

    // Bob needs an ATA (any payer may create it).
    let ata_ix = w.create_ata_ix(bob.pubkey(), alice.pubkey());
    w.send(&[ata_ix], &[&alice], &alice.pubkey()).unwrap();

    // Fail-closed: no ledger opened yet.
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "ledger-less gift passed"
    );

    // Fail-closed: extras omitted entirely.
    w.open_ledger(&alice);
    let bare = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, false);
    assert!(
        w.send(&[bare], &[&alice], &alice.pubkey()).is_err(),
        "hookless transfer passed"
    );

    // Within cap passes and the ledger tracks it.
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 30_000, true);
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.ledger_state(alice.pubkey()).gifted_today, 30_000);

    // Exactly to the cap passes; one more unit fails.
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        bob.pubkey(),
        CAP - 30_000,
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.ledger_state(alice.pubkey()).gifted_today, CAP);
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "cap breach passed"
    );

    // Day rollover resets the window.
    w.warp_days(1);
    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), bob.pubkey(), 1_000, true);
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
    assert_eq!(w.ledger_state(alice.pubkey()).gifted_today, 1_000);
}

#[test]
fn treasury_payments_bypass_the_cap_without_a_ledger() {
    let mut w = setup_with_guard();
    let alice = Keypair::new();
    w.svm.airdrop(&alice.pubkey(), 10_000_000_000).unwrap();
    w.earn(alice.pubkey(), 5_000);

    // No ledger opened, amount far above the gift cap — rule 2 short-circuits.
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        alice.pubkey(),
        w.merchant_authority.pubkey(),
        CAP * 4,
        true,
    );
    w.send(&[ix], &[&alice], &alice.pubkey()).unwrap();
}

#[test]
fn delegated_transfers_spend_the_source_owners_ledger() {
    let mut w = setup_with_guard();
    let alice = Keypair::new();
    let bob = Keypair::new();
    let delegate = Keypair::new();
    w.svm.airdrop(&alice.pubkey(), 10_000_000_000).unwrap();
    w.svm.airdrop(&delegate.pubkey(), 5_000_000_000).unwrap();
    w.earn(alice.pubkey(), 5_000);
    w.open_ledger(&alice);
    let ata_ix = w.create_ata_ix(bob.pubkey(), alice.pubkey());
    w.send(&[ata_ix], &[&alice], &alice.pubkey()).unwrap();

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

    // The delegate signs, but the ledger is derived from ALICE's owner field —
    // a delegate cannot mint themselves a fresh cap.
    let ix = w.hooked_transfer_ix(
        alice.pubkey(),
        delegate.pubkey(),
        bob.pubkey(),
        25_000,
        true,
    );
    w.send(&[ix], &[&delegate], &delegate.pubkey()).unwrap();
    assert_eq!(w.ledger_state(alice.pubkey()).gifted_today, 25_000);
}

#[test]
fn program_owned_destinations_are_rejected() {
    let mut w = setup_with_guard();
    let alice = Keypair::new();
    w.svm.airdrop(&alice.pubkey(), 10_000_000_000).unwrap();
    w.earn(alice.pubkey(), 5_000);
    w.open_ledger(&alice);

    // The EAML account is owned by argus — a stand-in for any pool/vault
    // authority that is not a plain system wallet.
    let pool_authority = w.eaml;
    let ata_ix = w.create_ata_ix(pool_authority, alice.pubkey());
    w.send(&[ata_ix], &[&alice], &alice.pubkey()).unwrap();

    let ix = w.hooked_transfer_ix(alice.pubkey(), alice.pubkey(), pool_authority, 1_000, true);
    assert!(
        w.send(&[ix], &[&alice], &alice.pubkey()).is_err(),
        "program-owned destination accepted"
    );
}

#[test]
fn argus_hardcoded_constants_match_vesta_core() {
    use anchor_lang::Discriminator;
    assert_eq!(argus::constants::VESTA_CORE_ID, vesta_core::id());
    assert_eq!(
        argus::constants::MERCHANT_DISCRIMINATOR,
        vesta_core::state::Merchant::DISCRIMINATOR
    );
}
