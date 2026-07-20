#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::too_many_arguments
)]

use {
    anchor_lang::{
        prelude::{Clock, Pubkey},
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
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
            ACHIEVE_SEED, BADGE_SEED, CAMPAIGN_SEED, CONFIG_SEED, CUSTOMER_SEED, KLEOS_SEED,
            MERCHANT_SEED, MINT_SEED,
        },
        state::{Achievement, CustomerProfile, Merchant},
        RegisterMerchantArgs,
    },
};

const TOKEN_2022_ID: Pubkey = spl_token_2022_interface::ID;
const ATA_PROGRAM_ID: Pubkey =
    Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

fn core_bytes() -> &'static [u8] {
    include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/vesta_core.so"
    ))
}

struct World {
    svm: LiteSVM,
    config: Pubkey,
    merchant_authority: Keypair,
    merchant: Pubkey,
    mint: Pubkey,
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

    fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }

    fn warp_days(&mut self, days: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp += days * 86_400;
        self.svm.set_sysvar::<Clock>(&clock);
    }

    fn profile_pda(&self, customer: Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[CUSTOMER_SEED, self.merchant.as_ref(), customer.as_ref()],
            &vesta_core::id(),
        )
        .0
    }

    fn campaign_progress_pda(&self, campaign: Pubkey, customer: Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[b"cprogress", campaign.as_ref(), customer.as_ref()],
            &vesta_core::id(),
        )
        .0
    }

    /// Plain streak-only earn.
    fn earn(
        &mut self,
        customer: Pubkey,
        amount_base: u64,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let visit_day = (self.now() / 86_400) as u32;
        let ata =
            get_associated_token_address_with_program_id(&customer, &self.mint, &TOKEN_2022_ID);
        let authority = self.merchant_authority.insecure_clone();
        let ix = Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::EarnPoints {
                merchant_authority: authority.pubkey(),
                merchant: self.merchant,
                customer,
                customer_profile: self.profile_pda(customer),
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
    }

    /// Governed campaign earn (signer may be an operator).
    fn earn_campaign(
        &mut self,
        customer: Pubkey,
        amount_base: u64,
        campaign: Pubkey,
        signer: &Keypair,
    ) -> Result<(), Box<litesvm::types::FailedTransactionMetadata>> {
        let visit_day = (self.now() / 86_400) as u32;
        let ata =
            get_associated_token_address_with_program_id(&customer, &self.mint, &TOKEN_2022_ID);
        let ix = Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::EarnPointsCampaign {
                merchant_authority: signer.pubkey(),
                merchant: self.merchant,
                customer,
                customer_profile: self.profile_pda(customer),
                campaign,
                campaign_progress: self.campaign_progress_pda(campaign, customer),
                point_mint: self.mint,
                customer_ata: ata,
                config: self.config,
                token_program: TOKEN_2022_ID,
                associated_token_program: ATA_PROGRAM_ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::EarnPointsCampaign {
                amount_base,
                visit_day,
            }
            .data(),
        };
        self.send(&[ix], &[signer], &signer.pubkey())
    }

    fn balance(&self, customer: Pubkey) -> u64 {
        let ata =
            get_associated_token_address_with_program_id(&customer, &self.mint, &TOKEN_2022_ID);
        let data = self.svm.get_account(&ata).unwrap().data;
        StateWithExtensions::<TokenAccount>::unpack(&data)
            .unwrap()
            .base
            .amount
    }

    fn campaign_pda(&self, id: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[CAMPAIGN_SEED, self.merchant.as_ref(), &id.to_le_bytes()],
            &vesta_core::id(),
        )
        .0
    }

    fn create_campaign(
        &mut self,
        id: u64,
        multiplier_bps: u16,
        starts_at: i64,
        ends_at: i64,
    ) -> Result<Pubkey, Box<litesvm::types::FailedTransactionMetadata>> {
        self.create_campaign_args(
            id,
            vesta_core::CampaignArgs {
                kind: 0, // MULTIPLIER
                multiplier_bps,
                flat_bonus: 0,
                quest_target: 0,
                quest_reward: 0,
                min_spend_base: 0,
                min_tier: 0,
                points_budget: 0,
                per_customer_cap: 0,
                starts_at,
                ends_at,
                name: "Boost".into(),
            },
        )
    }

    fn create_campaign_args(
        &mut self,
        id: u64,
        args: vesta_core::CampaignArgs,
    ) -> Result<Pubkey, Box<litesvm::types::FailedTransactionMetadata>> {
        let authority = self.merchant_authority.insecure_clone();
        let campaign = self.campaign_pda(id);
        let ix = Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::CreateCampaign {
                authority: authority.pubkey(),
                merchant: self.merchant,
                campaign,
                config: self.config,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::CreateCampaign { id, args }.data(),
        };
        self.send(&[ix], &[&authority], &authority.pubkey())?;
        Ok(campaign)
    }

    fn achievement_pdas(&self, id: u64, customer: Pubkey) -> (Pubkey, Pubkey, Pubkey, Pubkey) {
        let achievement = Pubkey::find_program_address(
            &[ACHIEVE_SEED, self.merchant.as_ref(), &id.to_le_bytes()],
            &vesta_core::id(),
        )
        .0;
        let badge_mint = Pubkey::find_program_address(
            &[BADGE_SEED, achievement.as_ref(), customer.as_ref()],
            &vesta_core::id(),
        )
        .0;
        let kleos = Pubkey::find_program_address(
            &[KLEOS_SEED, achievement.as_ref(), customer.as_ref()],
            &vesta_core::id(),
        )
        .0;
        let badge_ata =
            get_associated_token_address_with_program_id(&customer, &badge_mint, &TOKEN_2022_ID);
        (achievement, badge_mint, kleos, badge_ata)
    }

    fn grant_ix(&self, id: u64, customer: Pubkey, signer: Pubkey) -> Instruction {
        let (achievement, badge_mint, kleos, badge_ata) = self.achievement_pdas(id, customer);
        let profile = Pubkey::find_program_address(
            &[CUSTOMER_SEED, self.merchant.as_ref(), customer.as_ref()],
            &vesta_core::id(),
        )
        .0;
        Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::GrantAchievement {
                merchant_authority: signer,
                merchant: self.merchant,
                achievement,
                customer,
                customer_profile: profile,
                badge_mint,
                badge_ata,
                kleos_receipt: kleos,
                config: self.config,
                token_program: TOKEN_2022_ID,
                associated_token_program: ATA_PROGRAM_ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::GrantAchievement {}.data(),
        }
    }
}

fn setup() -> World {
    let mut svm = LiteSVM::new();
    svm.add_program(vesta_core::id(), core_bytes()).unwrap();
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

    let mut w = World {
        svm,
        config,
        merchant_authority,
        merchant,
        mint,
    };

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
                treasury,
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

#[test]
fn campaign_multiplier_applies_and_joint_cap_holds() {
    let mut w = setup();
    let auth = w.merchant_authority.insecure_clone();
    let customer = Keypair::new().pubkey();
    let now = w.now();

    // Windows and bounds are validated.
    assert!(
        w.create_campaign(9, 25_000, now, now + 86_400).is_err(),
        "bps over cap accepted"
    );
    assert!(
        w.create_campaign(9, 5_000, now + 100, now).is_err(),
        "inverted window accepted"
    );

    let campaign = w
        .create_campaign(1, 10_000, now - 60, now + 40 * 86_400)
        .unwrap();

    // Day 1, streak 1 (200 bps) + campaign 10_000 bps → 20_200 bps.
    // 100 * 100 * 2.02 = 20_200 raw.
    w.earn_campaign(customer, 100, campaign, &auth).unwrap();
    assert_eq!(w.balance(customer), 20_200);

    // Joint cap: push the streak to 30 days (16_000 bps), campaign 10_000 →
    // raw sum 26_000 capped at 24_000 bps → 100 * 100 * 2.4 = 24_000.
    for _ in 0..31 {
        w.warp_days(1);
        w.earn(customer, 1).unwrap();
    }
    let profile_pda = Pubkey::find_program_address(
        &[CUSTOMER_SEED, w.merchant.as_ref(), customer.as_ref()],
        &vesta_core::id(),
    )
    .0;
    let data = w.svm.get_account(&profile_pda).unwrap().data;
    let profile = CustomerProfile::try_deserialize(&mut data.as_slice()).unwrap();
    assert!(profile.streak_days >= 30);

    let before = w.balance(customer);
    w.earn_campaign(customer, 100, campaign, &auth).unwrap();
    assert_eq!(
        w.balance(customer) - before,
        24_000,
        "joint multiplier cap not enforced"
    );
}

#[test]
fn campaign_rejects_wrong_merchant_expired_and_closed() {
    let mut w = setup();
    let auth = w.merchant_authority.insecure_clone();
    let customer = Keypair::new().pubkey();
    let now = w.now();

    // Expired window.
    let expired = w.create_campaign(2, 5_000, now - 1_000, now - 10).unwrap();
    assert!(
        w.earn_campaign(customer, 100, expired, &auth).is_err(),
        "expired campaign applied"
    );

    // Not started yet.
    let future = w
        .create_campaign(3, 5_000, now + 1_000, now + 2_000)
        .unwrap();
    assert!(
        w.earn_campaign(customer, 100, future, &auth).is_err(),
        "future campaign applied"
    );

    // Another merchant's campaign.
    let other_authority = Keypair::new();
    w.svm
        .airdrop(&other_authority.pubkey(), 50_000_000_000)
        .unwrap();
    let other_merchant = Pubkey::find_program_address(
        &[MERCHANT_SEED, other_authority.pubkey().as_ref()],
        &vesta_core::id(),
    )
    .0;
    let other_mint =
        Pubkey::find_program_address(&[MINT_SEED, other_merchant.as_ref()], &vesta_core::id()).0;
    let other_treasury = get_associated_token_address_with_program_id(
        &other_authority.pubkey(),
        &other_mint,
        &TOKEN_2022_ID,
    );
    w.send(
        &[Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::RegisterMerchant {
                authority: other_authority.pubkey(),
                merchant: other_merchant,
                mint: other_mint,
                treasury: other_treasury,
                config: w.config,
                token_program: TOKEN_2022_ID,
                associated_token_program: ATA_PROGRAM_ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::RegisterMerchant {
                args: RegisterMerchantArgs {
                    name: "Litera".into(),
                    symbol: "BKS".into(),
                    uri: "https://vesta.example/books.json".into(),
                    decay_rate_bps: -2_000,
                    base_earn_rate: 100,
                    decimals: 2,
                },
            }
            .data(),
        }],
        &[&other_authority],
        &other_authority.pubkey(),
    )
    .unwrap();
    let foreign = Pubkey::find_program_address(
        &[CAMPAIGN_SEED, other_merchant.as_ref(), &7u64.to_le_bytes()],
        &vesta_core::id(),
    )
    .0;
    let ix = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::CreateCampaign {
            authority: other_authority.pubkey(),
            merchant: other_merchant,
            campaign: foreign,
            config: w.config,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::CreateCampaign {
            id: 7,
            args: vesta_core::CampaignArgs {
                kind: 0,
                multiplier_bps: 20_000,
                flat_bonus: 0,
                quest_target: 0,
                quest_reward: 0,
                min_spend_base: 0,
                min_tier: 0,
                points_budget: 0,
                per_customer_cap: 0,
                starts_at: now - 60,
                ends_at: now + 86_400,
                name: "Foreign".into(),
            },
        }
        .data(),
    };
    w.send(&[ix], &[&other_authority], &other_authority.pubkey())
        .unwrap();
    assert!(
        w.earn_campaign(customer, 100, foreign, &auth).is_err(),
        "foreign merchant campaign applied"
    );

    // Closed campaign account is gone entirely.
    let open = w.create_campaign(4, 5_000, now - 60, now + 86_400).unwrap();
    let authority = w.merchant_authority.insecure_clone();
    let close = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::CloseCampaign {
            authority: authority.pubkey(),
            merchant: w.merchant,
            campaign: open,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::CloseCampaign {}.data(),
    };
    w.send(&[close], &[&authority], &authority.pubkey())
        .unwrap();
    assert!(w.svm.get_account(&open).is_none());
    assert!(
        w.earn_campaign(customer, 100, open, &auth).is_err(),
        "closed campaign applied"
    );
}

#[test]
fn kleos_badge_full_lifecycle_and_burn_proof_guard() {
    let mut w = setup();
    let customer = Keypair::new();
    w.svm.airdrop(&customer.pubkey(), 10_000_000_000).unwrap();
    let authority = w.merchant_authority.insecure_clone();

    // Definition: threshold 100_000 raw lifetime.
    let (achievement, badge_mint, kleos, badge_ata) = w.achievement_pdas(1, customer.pubkey());
    let create = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::CreateAchievement {
            authority: authority.pubkey(),
            merchant: w.merchant,
            achievement,
            config: w.config,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::CreateAchievement {
            id: 1,
            name: "First Flame".into(),
            uri: "https://vesta.example/badges/first-flame.json".into(),
            threshold_lifetime: 100_000,
        }
        .data(),
    };
    w.send(&[create], &[&authority], &authority.pubkey())
        .unwrap();

    // Below threshold → rejected.
    w.earn(customer.pubkey(), 100).unwrap();
    let grant = w.grant_ix(1, customer.pubkey(), authority.pubkey());
    assert!(
        w.send(
            std::slice::from_ref(&grant),
            &[&authority],
            &authority.pubkey()
        )
        .is_err(),
        "granted below threshold"
    );

    // Unauthorized signer → rejected (their merchant PDA does not exist).
    let rando = Keypair::new();
    w.svm.airdrop(&rando.pubkey(), 5_000_000_000).unwrap();
    let forged = w.grant_ix(1, customer.pubkey(), rando.pubkey());
    assert!(
        w.send(&[forged], &[&rando], &rando.pubkey()).is_err(),
        "forged grant accepted"
    );

    // Cross the threshold, grant for real.
    w.earn(customer.pubkey(), 5_000)
        .unwrap();
    w.send(
        std::slice::from_ref(&grant),
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();

    // Badge: NonTransferable + metadata, decimals 0, supply 1, authority burned.
    let mint_data = w.svm.get_account(&badge_mint).unwrap().data;
    let state = StateWithExtensions::<MintState>::unpack(&mint_data).unwrap();
    assert_eq!(state.base.decimals, 0);
    assert_eq!(state.base.supply, 1);
    assert!(
        state.base.mint_authority.is_none(),
        "badge mint authority not revoked"
    );
    use spl_token_2022_interface::extension::ExtensionType as E;
    let exts = state.get_extension_types().unwrap();
    assert!(exts.contains(&E::NonTransferable));
    assert!(exts.contains(&E::TokenMetadata));
    assert!(w.svm.get_account(&kleos).is_some(), "kleos receipt missing");

    let achievement_data = w.svm.get_account(&achievement).unwrap().data;
    let a = Achievement::try_deserialize(&mut achievement_data.as_slice()).unwrap();
    assert_eq!(a.badge_count, 1);
    // Merchant-level badge stat is wired.
    let m_data = w.svm.get_account(&w.merchant).unwrap().data;
    let m = Merchant::try_deserialize(&mut m_data.as_slice()).unwrap();
    assert_eq!(m.badges_issued, 1);

    // Soulbound: transferring the badge fails even to a fresh ATA.
    let friend = Keypair::new();
    let friend_ata =
        get_associated_token_address_with_program_id(&friend.pubkey(), &badge_mint, &TOKEN_2022_ID);
    let create_ata = spl_associated_token_account_interface::instruction::create_associated_token_account_idempotent(
        &customer.pubkey(),
        &friend.pubkey(),
        &badge_mint,
        &TOKEN_2022_ID,
    );
    let transfer = spl_token_2022_interface::instruction::transfer_checked(
        &TOKEN_2022_ID,
        &badge_ata,
        &badge_mint,
        &friend_ata,
        &customer.pubkey(),
        &[],
        1,
        0,
    )
    .unwrap();
    assert!(
        w.send(&[create_ata, transfer], &[&customer], &customer.pubkey())
            .is_err(),
        "soulbound badge transferred"
    );

    // Double grant → rejected while held.
    assert!(
        w.send(
            std::slice::from_ref(&grant),
            &[&authority],
            &authority.pubkey()
        )
        .is_err(),
        "double grant accepted"
    );

    // The money test: holder burns the badge and closes the ATA —
    // the KleosReceipt still blocks a re-grant.
    let burn = spl_token_2022_interface::instruction::burn(
        &TOKEN_2022_ID,
        &badge_ata,
        &badge_mint,
        &customer.pubkey(),
        &[],
        1,
    )
    .unwrap();
    let close = spl_token_2022_interface::instruction::close_account(
        &TOKEN_2022_ID,
        &badge_ata,
        &customer.pubkey(),
        &customer.pubkey(),
        &[],
    )
    .unwrap();
    w.send(&[burn, close], &[&customer], &customer.pubkey())
        .unwrap();
    assert!(
        w.svm.get_account(&badge_ata).is_none(),
        "badge ATA not closed"
    );
    assert!(
        w.send(
            std::slice::from_ref(&grant),
            &[&authority],
            &authority.pubkey()
        )
        .is_err(),
        "re-grant after burn accepted — KleosReceipt guard failed"
    );
}

// ── enriched campaigns (flat bonus, quest, budget/caps, eligibility) ─────────

fn args(
    kind: u8,
    multiplier_bps: u16,
    flat_bonus: u64,
    quest_target: u16,
    quest_reward: u64,
    min_spend_base: u64,
    min_tier: u8,
    points_budget: u64,
    per_customer_cap: u64,
    starts_at: i64,
    ends_at: i64,
) -> vesta_core::CampaignArgs {
    vesta_core::CampaignArgs {
        kind,
        multiplier_bps,
        flat_bonus,
        quest_target,
        quest_reward,
        min_spend_base,
        min_tier,
        points_budget,
        per_customer_cap,
        starts_at,
        ends_at,
        name: "C".into(),
    }
}

#[test]
fn campaign_flat_bonus_respects_budget_and_per_customer_cap() {
    let mut w = setup();
    let auth = w.merchant_authority.insecure_clone();
    let alice = Keypair::new().pubkey();
    let bob = Keypair::new().pubkey();
    let now = w.now();
    // FLAT_BONUS 5_000, total budget 8_000, per-customer cap 5_000.
    let c = w
        .create_campaign_args(10, args(1, 0, 5_000, 0, 0, 0, 0, 8_000, 5_000, now - 60, now + 86_400))
        .unwrap();

    // Alice: base (streak 1) 10_200 + flat 5_000.
    w.earn_campaign(alice, 100, c, &auth).unwrap();
    assert_eq!(w.balance(alice), 15_200);
    // Alice again (same day): per-customer cap exhausted → base only.
    w.earn_campaign(alice, 100, c, &auth).unwrap();
    assert_eq!(w.balance(alice), 15_200 + 10_200);
    // Bob: budget remaining is 3_000 → flat clamped to 3_000.
    w.earn_campaign(bob, 100, c, &auth).unwrap();
    assert_eq!(w.balance(bob), 10_200 + 3_000);
}

#[test]
fn campaign_quest_pays_reward_once() {
    let mut w = setup();
    let auth = w.merchant_authority.insecure_clone();
    let alice = Keypair::new().pubkey();
    let now = w.now();
    // QUEST: 2 qualifying visits → 7_000 reward.
    let c = w
        .create_campaign_args(11, args(2, 0, 0, 2, 7_000, 0, 0, 0, 0, now - 60, now + 40 * 86_400))
        .unwrap();

    // Visit 1 (streak 1): base 10_200, no reward yet.
    w.earn_campaign(alice, 100, c, &auth).unwrap();
    assert_eq!(w.balance(alice), 10_200);
    // Visit 2 (next day, streak 2): base 10_400 + reward 7_000.
    w.warp_days(1);
    let b = w.balance(alice);
    w.earn_campaign(alice, 100, c, &auth).unwrap();
    assert_eq!(w.balance(alice) - b, 10_400 + 7_000);
    // Visit 3 (streak 3): base 10_600, no further reward.
    w.warp_days(1);
    let b = w.balance(alice);
    w.earn_campaign(alice, 100, c, &auth).unwrap();
    assert_eq!(w.balance(alice) - b, 10_600);
}

#[test]
fn campaign_gates_on_min_spend() {
    let mut w = setup();
    let auth = w.merchant_authority.insecure_clone();
    let alice = Keypair::new().pubkey();
    let now = w.now();
    let c = w
        .create_campaign_args(12, args(1, 0, 5_000, 0, 0, 1_000, 0, 0, 0, now - 60, now + 86_400))
        .unwrap();
    // Below min spend → rejected.
    assert!(
        w.earn_campaign(alice, 100, c, &auth).is_err(),
        "under-spend earn applied the campaign"
    );
    // At the threshold → applies (base 1000*100*1.02=102_000 + flat 5_000).
    w.earn_campaign(alice, 1_000, c, &auth).unwrap();
    assert_eq!(w.balance(alice), 102_000 + 5_000);
}

fn set_operator_ix(w: &World, owner: &Keypair, operator: Pubkey, add: bool) -> Instruction {
    Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::MerchantOwnerOnly {
            authority: owner.pubkey(),
            merchant: w.merchant,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::SetMerchantOperator { operator, add }.data(),
    }
}

#[test]
fn merchant_operator_earns_and_pause_blocks() {
    let mut w = setup();
    let owner = w.merchant_authority.insecure_clone();
    let operator = Keypair::new();
    let rando = Keypair::new();
    w.svm.airdrop(&operator.pubkey(), 5_000_000_000).unwrap();
    w.svm.airdrop(&rando.pubkey(), 5_000_000_000).unwrap();
    let alice = Keypair::new().pubkey();
    let now = w.now();
    let c = w.create_campaign(20, 5_000, now - 60, now + 86_400).unwrap();

    // Grant the operator.
    let ix = set_operator_ix(&w, &owner, operator.pubkey(), true);
    w.send(&[ix], &[&owner], &owner.pubkey()).unwrap();

    // Operator can run a campaign earn.
    w.earn_campaign(alice, 100, c, &operator).unwrap();
    assert!(w.balance(alice) > 0);

    // A random signer cannot.
    assert!(
        w.earn_campaign(alice, 100, c, &rando).is_err(),
        "non-operator ran earn"
    );

    // Revoke the operator → it can no longer earn.
    let ix = set_operator_ix(&w, &owner, operator.pubkey(), false);
    w.send(&[ix], &[&owner], &owner.pubkey()).unwrap();
    assert!(
        w.earn_campaign(alice, 100, c, &operator).is_err(),
        "revoked operator still earned"
    );

    // Pausing the merchant blocks even the owner.
    let pause = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::MerchantOwnerOnly {
            authority: owner.pubkey(),
            merchant: w.merchant,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::SetMerchantPaused { paused: true }.data(),
    };
    w.send(&[pause], &[&owner], &owner.pubkey()).unwrap();
    assert!(
        w.earn(alice, 100).is_err(),
        "paused merchant still earned"
    );
}

#[test]
fn quest_stays_open_when_budget_clamps_the_reward() {
    let mut w = setup();
    let auth = w.merchant_authority.insecure_clone();
    let alice = Keypair::new().pubkey();
    let now = w.now();
    // QUEST: 1 visit → 5_000 reward, but total budget only 3_000.
    let c = w
        .create_campaign_args(30, args(2, 0, 0, 1, 5_000, 0, 0, 3_000, 0, now - 60, now + 40 * 86_400))
        .unwrap();

    // First (target-reaching) visit: reward clamped to 3_000 → quest NOT completed.
    w.earn_campaign(alice, 100, c, &auth).unwrap();
    let prog = Pubkey::find_program_address(
        &[b"cprogress", c.as_ref(), alice.as_ref()],
        &vesta_core::id(),
    )
    .0;
    let data = w.svm.get_account(&prog).unwrap().data;
    let p = vesta_core::state::CampaignProgress::try_deserialize(&mut data.as_slice()).unwrap();
    assert!(!p.completed, "quest completed on a clamped (partial) payout");
    assert_eq!(p.bonus_drawn, 3_000);
}

#[test]
fn close_achievement_reclaims_and_removes_definition() {
    let mut w = setup();
    let authority = w.merchant_authority.insecure_clone();
    let (achievement, _, _, _) = w.achievement_pdas(9, Keypair::new().pubkey());
    let create = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::CreateAchievement {
            authority: authority.pubkey(),
            merchant: w.merchant,
            achievement,
            config: w.config,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::CreateAchievement {
            id: 9,
            name: "Legacy".into(),
            uri: "https://vesta.example/legacy.json".into(),
            threshold_lifetime: 100,
        }
        .data(),
    };
    w.send(&[create], &[&authority], &authority.pubkey()).unwrap();
    assert!(w.svm.get_account(&achievement).is_some());

    let close = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::CloseAchievement {
            authority: authority.pubkey(),
            merchant: w.merchant,
            achievement,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::CloseAchievement {}.data(),
    };
    w.send(&[close], &[&authority], &authority.pubkey()).unwrap();
    assert!(w.svm.get_account(&achievement).is_none_or(|a| a.lamports == 0));
}
