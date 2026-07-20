#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use {
    anchor_lang::{
        prelude::{Clock, Pubkey},
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, Discriminator, InstructionData, ToAccountMetas,
    },
    anchor_spl::associated_token::get_associated_token_address_with_program_id,
    litesvm::LiteSVM,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    spl_token_2022_interface::{
        extension::{BaseStateWithExtensions, StateWithExtensions},
        state::{Account as TokenAccount, Mint as MintState},
    },
    vesta_core::{
        constants::{
            CONFIG_SEED, CUSTOMER_SEED, MERCHANT_SEED, MINT_SEED, OFFER_SEED, RECEIPT_SEED,
        },
        state::{Config, CustomerProfile, Merchant, Offer},
        RegisterMerchantArgs,
    },
};

const TOKEN_2022_ID: Pubkey = spl_token_2022_interface::ID;
const ATA_PROGRAM_ID: Pubkey =
    Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

fn program_bytes() -> &'static [u8] {
    include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/vesta_core.so"
    ))
}

struct Harness {
    svm: LiteSVM,
    admin: Keypair,
    config: Pubkey,
}

fn pdas(program_id: &Pubkey, authority: &Pubkey) -> (Pubkey, Pubkey, Pubkey) {
    let merchant = Pubkey::find_program_address(
        &[MERCHANT_SEED, authority.as_ref(), &0u64.to_le_bytes()],
        program_id,
    )
    .0;
    let mint = Pubkey::find_program_address(&[MINT_SEED, merchant.as_ref()], program_id).0;
    let treasury = get_associated_token_address_with_program_id(authority, &mint, &TOKEN_2022_ID);
    (merchant, mint, treasury)
}

impl Harness {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        svm.add_program(vesta_core::id(), program_bytes()).unwrap();
        // LiteSVM's clock starts at 0 — pin a realistic timestamp so day math behaves.
        let mut clock = svm.get_sysvar::<Clock>();
        clock.unix_timestamp = 1_760_000_000;
        svm.set_sysvar::<Clock>(&clock);
        let admin = Keypair::new();
        svm.airdrop(&admin.pubkey(), 100_000_000_000).unwrap();
        let config = Pubkey::find_program_address(&[CONFIG_SEED], &vesta_core::id()).0;

        let mut h = Harness { svm, admin, config };
        h.send(
            &[h.ix(
                vesta_core::accounts::InitConfig {
                    admin: h.admin.pubkey(),
                    config,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                vesta_core::instruction::InitConfig {}.data(),
            )],
            &[&h.admin.insecure_clone()],
            &h.admin.pubkey(),
        )
        .unwrap();
        h
    }

    fn ix(
        &self,
        accounts: Vec<anchor_lang::solana_program::instruction::AccountMeta>,
        data: Vec<u8>,
    ) -> Instruction {
        Instruction {
            program_id: vesta_core::id(),
            accounts,
            data,
        }
    }

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

    fn register_merchant(&mut self, authority: &Keypair, name: &str) -> (Pubkey, Pubkey, Pubkey) {
        self.svm
            .airdrop(&authority.pubkey(), 10_000_000_000)
            .unwrap();
        let (merchant, mint, treasury) = pdas(&vesta_core::id(), &authority.pubkey());
        self.send(
            &[self.ix(
                vesta_core::accounts::RegisterMerchant {
                    authority: authority.pubkey(),
                    merchant,
                    mint,
                    treasury,
                    config: self.config,
                    token_program: TOKEN_2022_ID,
                    associated_token_program: ATA_PROGRAM_ID,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                vesta_core::instruction::RegisterMerchant {
                    id: 0,
                    args: RegisterMerchantArgs {
                        name: name.into(),
                        symbol: "PTS".into(),
                        uri: "https://vesta.example/points.json".into(),
                        decay_rate_bps: -2_000,
                        base_earn_rate: 100,
                        decimals: 2,
                    },
                }
                .data(),
            )],
            &[authority],
            &authority.pubkey(),
        )
        .unwrap();
        (merchant, mint, treasury)
    }

    fn earn(
        &mut self,
        authority: &Keypair,
        merchant: Pubkey,
        mint: Pubkey,
        customer: Pubkey,
        amount_base: u64,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let visit_day = (self.clock().unix_timestamp / 86_400) as u32;
        self.earn_on_day(authority, merchant, mint, customer, amount_base, visit_day)
    }

    fn earn_on_day(
        &mut self,
        authority: &Keypair,
        merchant: Pubkey,
        mint: Pubkey,
        customer: Pubkey,
        amount_base: u64,
        visit_day: u32,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let profile = Pubkey::find_program_address(
            &[CUSTOMER_SEED, merchant.as_ref(), customer.as_ref()],
            &vesta_core::id(),
        )
        .0;
        let ata = get_associated_token_address_with_program_id(&customer, &mint, &TOKEN_2022_ID);
        self.send(
            &[self.ix(
                vesta_core::accounts::EarnPoints {
                    merchant_authority: authority.pubkey(),
                    merchant,
                    customer,
                    customer_profile: profile,
                    point_mint: mint,
                    customer_ata: ata,
                    config: self.config,
                    token_program: TOKEN_2022_ID,
                    associated_token_program: ATA_PROGRAM_ID,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                vesta_core::instruction::EarnPoints {
                    amount_base,
                    visit_day,
                }
                .data(),
            )],
            &[authority],
            &authority.pubkey(),
        )
    }

    fn clock(&self) -> Clock {
        self.svm.get_sysvar::<Clock>()
    }

    fn warp_days(&mut self, days: i64) {
        let mut clock = self.clock();
        clock.unix_timestamp += days * 86_400;
        self.svm.set_sysvar::<Clock>(&clock);
    }

    fn token_balance(&self, ata: &Pubkey) -> u64 {
        let data = self.svm.get_account(ata).unwrap().data;
        StateWithExtensions::<TokenAccount>::unpack(&data)
            .unwrap()
            .base
            .amount
    }

    fn account<T: AccountDeserialize>(&self, key: &Pubkey) -> T {
        let data = self.svm.get_account(key).unwrap().data;
        T::try_deserialize(&mut data.as_slice()).unwrap()
    }
}

#[test]
fn register_merchant_composes_full_extension_stack() {
    let mut h = Harness::new();
    let authority = Keypair::new();
    let (merchant_pda, mint, treasury) = h.register_merchant(&authority, "Kavarna");

    let merchant: Merchant = h.account(&merchant_pda);
    assert_eq!(merchant.authority, authority.pubkey());
    assert_eq!(merchant.point_mint, mint);
    assert_eq!(merchant.treasury, treasury);
    assert_eq!(merchant.decay_rate_bps, -2_000);

    let mint_data = h.svm.get_account(&mint).unwrap();
    assert_eq!(mint_data.owner, TOKEN_2022_ID);
    let state = StateWithExtensions::<MintState>::unpack(&mint_data.data).unwrap();
    assert_eq!(state.base.decimals, 2);
    let exts = state.get_extension_types().unwrap();
    use spl_token_2022_interface::extension::ExtensionType as E;
    for want in [
        E::MetadataPointer,
        E::InterestBearingConfig,
        E::TransferHook,
        E::PermanentDelegate,
        E::TokenMetadata,
    ] {
        assert!(exts.contains(&want), "missing extension {want:?}");
    }
    assert!(
        h.svm.get_account(&treasury).is_some(),
        "treasury ATA missing"
    );
}

#[test]
fn set_token_attribute_enriches_metadata() {
    use spl_token_2022_interface::extension::BaseStateWithExtensions;
    use spl_token_metadata_interface::state::TokenMetadata;

    let mut h = Harness::new();
    let authority = Keypair::new();
    let (merchant, mint, _) = h.register_merchant(&authority, "Kavarna");

    // Attach two custom attributes.
    for (k, v) in [("tier", "gold"), ("region", "EU")] {
        h.send(
            &[h.ix(
                vesta_core::accounts::SetTokenAttribute {
                    authority: authority.pubkey(),
                    merchant,
                    point_mint: mint,
                    config: h.config,
                    token_program: TOKEN_2022_ID,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                vesta_core::instruction::SetTokenAttribute {
                    key: k.into(),
                    value: v.into(),
                }
                .data(),
            )],
            &[&authority],
            &authority.pubkey(),
        )
        .unwrap();
    }

    // The metadata now carries both key/values.
    let mint_data = h.svm.get_account(&mint).unwrap().data;
    let state = StateWithExtensions::<MintState>::unpack(&mint_data).unwrap();
    let meta = state.get_variable_len_extension::<TokenMetadata>().unwrap();
    let got: std::collections::HashMap<_, _> = meta.additional_metadata.into_iter().collect();
    assert_eq!(got.get("tier").map(String::as_str), Some("gold"));
    assert_eq!(got.get("region").map(String::as_str), Some("EU"));

    // A non-authority cannot set attributes.
    let rando = Keypair::new();
    h.svm.airdrop(&rando.pubkey(), 1_000_000_000).unwrap();
    assert!(
        h.send(
            &[h.ix(
                vesta_core::accounts::SetTokenAttribute {
                    authority: rando.pubkey(),
                    merchant,
                    point_mint: mint,
                    config: h.config,
                    token_program: TOKEN_2022_ID,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                vesta_core::instruction::SetTokenAttribute {
                    key: "tier".into(),
                    value: "hacked".into(),
                }
                .data(),
            )],
            &[&rando],
            &rando.pubkey(),
        )
        .is_err(),
        "non-authority set an attribute"
    );
}

#[test]
fn register_merchant_survives_prefunded_mint_griefing() {
    let mut h = Harness::new();
    let authority = Keypair::new();
    let (_, mint, _) = pdas(&vesta_core::id(), &authority.pubkey());
    // Attacker donates 1 lamport to the predictable mint address before registration.
    h.svm.airdrop(&mint, 1).unwrap();
    h.register_merchant(&authority, "Griefed");
    let mint_data = h.svm.get_account(&mint).unwrap();
    assert_eq!(mint_data.owner, TOKEN_2022_ID);
}

#[test]
fn register_merchant_rejects_bad_args() {
    let mut h = Harness::new();
    let authority = Keypair::new();
    h.svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();
    let (merchant, mint, treasury) = pdas(&vesta_core::id(), &authority.pubkey());

    let mut try_args = |args: RegisterMerchantArgs| {
        h.send(
            &[h.ix(
                vesta_core::accounts::RegisterMerchant {
                    authority: authority.pubkey(),
                    merchant,
                    mint,
                    treasury,
                    config: h.config,
                    token_program: TOKEN_2022_ID,
                    associated_token_program: ATA_PROGRAM_ID,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                vesta_core::instruction::RegisterMerchant { id: 0, args }.data(),
            )],
            &[&authority.insecure_clone()],
            &authority.pubkey(),
        )
    };

    let base = || RegisterMerchantArgs {
        name: "ok".into(),
        symbol: "OK".into(),
        uri: "https://ok".into(),
        decay_rate_bps: -2_000,
        base_earn_rate: 100,
        decimals: 2,
    };

    let mut bad_name = base();
    bad_name.name = "x".repeat(33);
    assert!(try_args(bad_name).is_err(), "oversized name accepted");

    let mut positive_rate = base();
    positive_rate.decay_rate_bps = 1;
    assert!(try_args(positive_rate).is_err(), "positive decay accepted");

    let mut zero_rate = base();
    zero_rate.base_earn_rate = 0;
    assert!(try_args(zero_rate).is_err(), "zero earn rate accepted");

    let mut bad_decimals = base();
    bad_decimals.decimals = 6;
    assert!(try_args(bad_decimals).is_err(), "wrong decimals accepted");
}

#[test]
fn earn_streak_and_tier_progression() {
    let mut h = Harness::new();
    let authority = Keypair::new();
    let (merchant, mint, _) = h.register_merchant(&authority, "Kavarna");
    let customer = Keypair::new().pubkey();
    let ata = get_associated_token_address_with_program_id(&customer, &mint, &TOKEN_2022_ID);

    // Day 1: streak 1 → 10_200 bps → 500 * 100 * 1.02 = 51_000 raw.
    h.earn(&authority, merchant, mint, customer, 500).unwrap();
    assert_eq!(h.token_balance(&ata), 51_000);

    let profile_pda = Pubkey::find_program_address(
        &[CUSTOMER_SEED, merchant.as_ref(), customer.as_ref()],
        &vesta_core::id(),
    )
    .0;
    let profile: CustomerProfile = h.account(&profile_pda);
    assert_eq!(profile.streak_days, 1);

    // Same-day repeat keeps the streak.
    h.earn(&authority, merchant, mint, customer, 100).unwrap();
    let profile: CustomerProfile = h.account(&profile_pda);
    assert_eq!(profile.streak_days, 1);

    // Next day bumps it.
    h.warp_days(1);
    h.earn(&authority, merchant, mint, customer, 100).unwrap();
    let profile: CustomerProfile = h.account(&profile_pda);
    assert_eq!(profile.streak_days, 2);

    // A gap resets to 1.
    h.warp_days(3);
    h.earn(&authority, merchant, mint, customer, 500).unwrap();
    let profile: CustomerProfile = h.account(&profile_pda);
    assert_eq!(profile.streak_days, 1);
    // 51_000 + 10_200 + 10_400 + 51_000 = 122_600 raw ≥ 100_000 → tier 1.
    assert_eq!(
        profile.tier, 1,
        "lifetime {} should reach tier 1",
        profile.lifetime_earned
    );

    let merchant_state: Merchant = h.account(&merchant);
    assert_eq!(merchant_state.customer_count, 1);
}

#[test]
fn earn_rejects_stale_day_forged_signer_cap_and_pause() {
    let mut h = Harness::new();
    let authority = Keypair::new();
    let (merchant, mint, _) = h.register_merchant(&authority, "Kavarna");
    let customer = Keypair::new().pubkey();

    let today = (h.clock().unix_timestamp / 86_400) as u32;
    assert!(
        h.earn_on_day(&authority, merchant, mint, customer, 100, today - 1)
            .is_err(),
        "stale visit_day accepted"
    );

    // Forged signer: a different wallet cannot earn against this merchant PDA.
    let forger = Keypair::new();
    h.svm.airdrop(&forger.pubkey(), 1_000_000_000).unwrap();
    assert!(
        h.earn(&forger, merchant, mint, customer, 100).is_err(),
        "forged merchant signer accepted"
    );

    // Per-tx cap: 500_000 base * 100 rate * 1.02 > 1_000_000 raw cap.
    assert!(
        h.earn(&authority, merchant, mint, customer, 500_000)
            .is_err(),
        "earn cap not enforced"
    );

    // Pause blocks earn.
    let admin = h.admin.insecure_clone();
    h.send(
        &[h.ix(
            vesta_core::accounts::AdminOnly {
                admin: admin.pubkey(),
                config: h.config,
            }
            .to_account_metas(None),
            vesta_core::instruction::SetPaused { paused: true }.data(),
        )],
        &[&admin],
        &admin.pubkey(),
    )
    .unwrap();
    assert!(
        h.earn(&authority, merchant, mint, customer, 100).is_err(),
        "paused earn accepted"
    );
}

#[test]
fn offer_lifecycle_redeem_decay_and_slippage() {
    let mut h = Harness::new();
    let authority = Keypair::new();
    let (merchant, mint, _) = h.register_merchant(&authority, "Kavarna");
    let customer = Keypair::new();
    h.svm.airdrop(&customer.pubkey(), 10_000_000_000).unwrap();
    h.earn(&authority, merchant, mint, customer.pubkey(), 5_000)
        .unwrap(); // 510_000 raw

    let offer = Pubkey::find_program_address(
        &[OFFER_SEED, merchant.as_ref(), &1u64.to_le_bytes()],
        &vesta_core::id(),
    )
    .0;
    h.send(
        &[h.ix(
            vesta_core::accounts::CreateOffer {
                authority: authority.pubkey(),
                merchant,
                offer,
                config: h.config,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            // 100.00 UI pts, supply 2
            vesta_core::instruction::CreateOffer {
                id: 1,
                price_points: 10_000,
                supply: 2,
            }
            .data(),
        )],
        &[&authority.insecure_clone()],
        &authority.pubkey(),
    )
    .unwrap();

    let profile = Pubkey::find_program_address(
        &[CUSTOMER_SEED, merchant.as_ref(), customer.pubkey().as_ref()],
        &vesta_core::id(),
    )
    .0;
    let ata =
        get_associated_token_address_with_program_id(&customer.pubkey(), &mint, &TOKEN_2022_ID);

    let redeem_ix = |h: &Harness, redemptions: u32, max_raw: u64| {
        let receipt = Pubkey::find_program_address(
            &[
                RECEIPT_SEED,
                offer.as_ref(),
                customer.pubkey().as_ref(),
                &redemptions.to_le_bytes(),
            ],
            &vesta_core::id(),
        )
        .0;
        (
            h.ix(
                vesta_core::accounts::RedeemOffer {
                    customer: customer.pubkey(),
                    merchant,
                    offer,
                    customer_profile: profile,
                    receipt,
                    point_mint: mint,
                    customer_ata: ata,
                    config: h.config,
                    token_program: TOKEN_2022_ID,
                    associated_token_program: ATA_PROGRAM_ID,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                vesta_core::instruction::RedeemOffer {
                    max_raw_amount: max_raw,
                }
                .data(),
            ),
            receipt,
        )
    };

    // Fresh mint: 100.00 UI ≈ 10_000 raw (tolerance for seconds of decay).
    let before = h.token_balance(&ata);
    let (ix, receipt) = redeem_ix(&h, 0, 11_000);
    h.send(&[ix], &[&customer], &customer.pubkey()).unwrap();
    let burned = before - h.token_balance(&ata);
    assert!(
        (10_000..=10_100).contains(&burned),
        "burned {burned}, expected ≈10_000"
    );

    let offer_state: Offer = h.account(&offer);
    assert_eq!(offer_state.supply_remaining, 1);

    // A year of decay: the same UI price now needs ~22% more raw.
    h.warp_days(365);
    let before = h.token_balance(&ata);
    let (ix, _) = redeem_ix(&h, 1, 10_100);
    assert!(
        h.send(&[ix], &[&customer], &customer.pubkey()).is_err(),
        "slippage bound not enforced after decay"
    );
    let (ix, _) = redeem_ix(&h, 1, 13_500);
    h.send(&[ix], &[&customer], &customer.pubkey()).unwrap();
    let burned = before - h.token_balance(&ata);
    assert!(
        burned > 11_500,
        "decay did not increase raw_needed: {burned}"
    );

    // Merchant-level redemption stat is wired (two successful redeems).
    let m: Merchant = h.account(&merchant);
    assert_eq!(m.lifetime_redemptions, 2);

    // A paused merchant cannot be redeemed against.
    let pause = h.ix(
        vesta_core::accounts::MerchantOwnerOnly {
            authority: authority.pubkey(),
            merchant,
        }
        .to_account_metas(None),
        vesta_core::instruction::SetMerchantPaused { paused: true }.data(),
    );
    h.send(
        &[pause],
        &[&authority.insecure_clone()],
        &authority.pubkey(),
    )
    .unwrap();
    let (ix, _) = redeem_ix(&h, 2, 20_000);
    assert!(
        h.send(&[ix], &[&customer], &customer.pubkey()).is_err(),
        "redeem on a paused merchant accepted"
    );
    // Unpause for the remaining assertions.
    let unpause = h.ix(
        vesta_core::accounts::MerchantOwnerOnly {
            authority: authority.pubkey(),
            merchant,
        }
        .to_account_metas(None),
        vesta_core::instruction::SetMerchantPaused { paused: false }.data(),
    );
    h.send(
        &[unpause],
        &[&authority.insecure_clone()],
        &authority.pubkey(),
    )
    .unwrap();

    // Supply exhausted now.
    let (ix, _) = redeem_ix(&h, 2, 20_000);
    assert!(
        h.send(&[ix], &[&customer], &customer.pubkey()).is_err(),
        "oversold offer"
    );

    // Receipt close returns rent to the customer.
    let lamports_before = h.svm.get_account(&customer.pubkey()).unwrap().lamports;
    h.send(
        &[h.ix(
            vesta_core::accounts::CloseReceipt {
                customer: customer.pubkey(),
                receipt,
            }
            .to_account_metas(None),
            vesta_core::instruction::CloseReceipt {}.data(),
        )],
        &[&customer],
        &customer.pubkey(),
    )
    .unwrap();
    assert!(h.svm.get_account(&receipt).is_none());
    assert!(h.svm.get_account(&customer.pubkey()).unwrap().lamports > lamports_before);
}

#[test]
fn admin_two_step_transfer_and_migration() {
    let mut h = Harness::new();

    // --- two-step admin transfer ---
    let new_admin = Keypair::new();
    h.svm.airdrop(&new_admin.pubkey(), 1_000_000_000).unwrap();
    let admin = h.admin.insecure_clone();
    h.send(
        &[h.ix(
            vesta_core::accounts::AdminOnly {
                admin: admin.pubkey(),
                config: h.config,
            }
            .to_account_metas(None),
            vesta_core::instruction::SetAdmin {
                new_admin: new_admin.pubkey(),
            }
            .data(),
        )],
        &[&admin],
        &admin.pubkey(),
    )
    .unwrap();

    // A random wallet cannot accept.
    let rando = Keypair::new();
    h.svm.airdrop(&rando.pubkey(), 1_000_000_000).unwrap();
    assert!(h
        .send(
            &[h.ix(
                vesta_core::accounts::AcceptAdmin {
                    pending_admin: rando.pubkey(),
                    config: h.config
                }
                .to_account_metas(None),
                vesta_core::instruction::AcceptAdmin {}.data(),
            )],
            &[&rando],
            &rando.pubkey(),
        )
        .is_err());

    h.send(
        &[h.ix(
            vesta_core::accounts::AcceptAdmin {
                pending_admin: new_admin.pubkey(),
                config: h.config,
            }
            .to_account_metas(None),
            vesta_core::instruction::AcceptAdmin {}.data(),
        )],
        &[&new_admin],
        &new_admin.pubkey(),
    )
    .unwrap();
    let config: Config = h.account(&h.config);
    assert_eq!(config.admin, new_admin.pubkey());
    assert_eq!(config.pending_admin, None);

    // --- v1 → v2 migration on a fabricated v1 account ---
    let mut svm = LiteSVM::new();
    svm.add_program(vesta_core::id(), program_bytes()).unwrap();
    let admin = Keypair::new();
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();
    let config_pda = Pubkey::find_program_address(&[CONFIG_SEED], &vesta_core::id()).0;

    let mut v1 = Vec::with_capacity(42);
    v1.extend_from_slice(Config::DISCRIMINATOR);
    v1.extend_from_slice(admin.pubkey().as_ref());
    v1.push(0); // paused = false
    v1.push(254); // bump placeholder
    svm.set_account(
        config_pda,
        solana_account::Account {
            lamports: 10_000_000,
            data: v1,
            owner: vesta_core::id(),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let mut h2 = Harness {
        svm,
        admin,
        config: config_pda,
    };
    let admin = h2.admin.insecure_clone();
    let migrate = h2.ix(
        vesta_core::accounts::MigrateConfig {
            admin: admin.pubkey(),
            config: config_pda,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        vesta_core::instruction::MigrateConfig {}.data(),
    );
    h2.send(std::slice::from_ref(&migrate), &[&admin], &admin.pubkey())
        .unwrap();

    let config: Config = h2.account(&config_pda);
    assert_eq!(config.admin, admin.pubkey());
    assert_eq!(config.pending_admin, None);
    assert!(!config.paused);

    // One-shot: a second run must fail.
    assert!(h2.send(&[migrate], &[&admin], &admin.pubkey()).is_err());
}

#[test]
fn metadata_and_decay_are_mutable() {
    use spl_token_2022_interface::extension::BaseStateWithExtensions;
    use spl_token_metadata_interface::state::TokenMetadata;

    let mut h = Harness::new();
    let authority = Keypair::new();
    let (merchant, mint, _) = h.register_merchant(&authority, "Kavarna");
    let auth = authority.insecure_clone();
    let cfg = h.config;

    let meta_ix = |kind: u8, value: &str| Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::SetTokenAttribute {
            authority: authority.pubkey(),
            merchant,
            point_mint: mint,
            config: cfg,
            token_program: TOKEN_2022_ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::UpdateTokenMetadata {
            field_kind: kind,
            value: value.to_string(),
        }
        .data(),
    };
    // Rebrand the token name.
    h.send(
        &[meta_ix(0, "Kavarna Rewards")],
        &[&auth],
        &authority.pubkey(),
    )
    .unwrap();
    let mint_data = h.svm.get_account(&mint).unwrap().data;
    let state = StateWithExtensions::<MintState>::unpack(&mint_data).unwrap();
    let md = state.get_variable_len_extension::<TokenMetadata>().unwrap();
    assert_eq!(md.name, "Kavarna Rewards");

    // Update the decay rate; merchant mirror updates and bounds are enforced.
    let decay_ix = |bps: i16| Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::SetTokenAttribute {
            authority: authority.pubkey(),
            merchant,
            point_mint: mint,
            config: cfg,
            token_program: TOKEN_2022_ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::UpdateDecayRate { new_rate_bps: bps }.data(),
    };
    h.send(&[decay_ix(-500)], &[&auth], &authority.pubkey())
        .unwrap();
    let m: Merchant = h.account(&merchant);
    assert_eq!(m.decay_rate_bps, -500);
    // Positive (inflationary) rate is rejected.
    assert!(
        h.send(&[decay_ix(500)], &[&auth], &authority.pubkey())
            .is_err(),
        "positive decay rate accepted"
    );
}

fn register_with_id(h: &mut Harness, authority: &Keypair, id: u64, name: &str) -> (Pubkey, Pubkey) {
    let merchant = Pubkey::find_program_address(
        &[
            MERCHANT_SEED,
            authority.pubkey().as_ref(),
            &id.to_le_bytes(),
        ],
        &vesta_core::id(),
    )
    .0;
    let mint = Pubkey::find_program_address(&[MINT_SEED, merchant.as_ref()], &vesta_core::id()).0;
    let treasury =
        get_associated_token_address_with_program_id(&authority.pubkey(), &mint, &TOKEN_2022_ID);
    let ix = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::RegisterMerchant {
            authority: authority.pubkey(),
            merchant,
            mint,
            treasury,
            config: h.config,
            token_program: TOKEN_2022_ID,
            associated_token_program: ATA_PROGRAM_ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::RegisterMerchant {
            id,
            args: RegisterMerchantArgs {
                name: name.into(),
                symbol: "PTS".into(),
                uri: "https://vesta.example/points.json".into(),
                decay_rate_bps: -2_000,
                base_earn_rate: 100,
                decimals: 2,
            },
        }
        .data(),
    };
    h.send(&[ix], &[authority], &authority.pubkey()).unwrap();
    (merchant, mint)
}

#[test]
fn wallet_owns_multiple_merchants_with_delete() {
    let mut h = Harness::new();
    let authority = Keypair::new();
    h.svm.airdrop(&authority.pubkey(), 30_000_000_000).unwrap();

    // One wallet, two distinct merchants (multi-record).
    let (m0, mint0) = register_with_id(&mut h, &authority, 0, "Cafe");
    let (m1, mint1) = register_with_id(&mut h, &authority, 1, "Bookstore");
    assert_ne!(m0, m1);
    assert_ne!(mint0, mint1);
    let a0: Merchant = h.account(&m0);
    let a1: Merchant = h.account(&m1);
    assert_eq!(a0.id, 0);
    assert_eq!(a1.id, 1);
    assert_eq!(a0.authority, authority.pubkey());
    assert_eq!(a1.authority, authority.pubkey());

    // Delete the empty second merchant → mint + merchant reclaimed.
    let close = |merchant: Pubkey, mint: Pubkey| Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::CloseMerchant {
            authority: authority.pubkey(),
            merchant,
            point_mint: mint,
            token_program: TOKEN_2022_ID,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::CloseMerchant {}.data(),
    };
    h.send(&[close(m1, mint1)], &[&authority], &authority.pubkey())
        .unwrap();
    assert!(h.svm.get_account(&m1).is_none_or(|a| a.lamports == 0));
    assert!(h.svm.get_account(&mint1).is_none_or(|a| a.lamports == 0));

    // The first merchant issues points → it can no longer be closed.
    let customer = Keypair::new().pubkey();
    h.earn(&authority, m0, mint0, customer, 100).unwrap();
    assert!(
        h.send(&[close(m0, mint0)], &[&authority], &authority.pubkey())
            .is_err(),
        "closed a merchant with circulating points"
    );
}
