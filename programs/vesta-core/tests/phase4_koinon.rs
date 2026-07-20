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
    spl_token_2022_interface::{extension::StateWithExtensions, state::Account as TokenAccount},
    vesta_core::{
        constants::{
            ALLIANCE_SEED, CONFIG_SEED, CUSTOMER_SEED, MEMBER_SEED, MERCHANT_SEED, MINT_SEED,
        },
        state::{Alliance, AllianceMember, CustomerProfile, Merchant},
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
fn argus_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/argus.so"))
}
fn aegis_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/aegis.so"))
}

/// SetComputeUnitLimit (discriminant 2) — the swap's two UiAmountToAmount
/// CPIs run float exp math and need more than the 200k default (spec §7.3).
fn cu_limit_ix(units: u32) -> Instruction {
    let mut data = vec![2u8];
    data.extend(units.to_le_bytes());
    Instruction {
        program_id: Pubkey::from_str_const("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data,
    }
}

struct Shop {
    authority: Keypair,
    merchant: Pubkey,
    mint: Pubkey,
    treasury: Pubkey,
}

struct World {
    svm: LiteSVM,
    config: Pubkey,
}

impl World {
    fn new() -> Self {
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
        let mut w = World { svm, config };
        w.send(
            &[Instruction {
                program_id: vesta_core::id(),
                accounts: vesta_core::accounts::InitConfig {
                    admin: admin.pubkey(),
                    config,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                data: vesta_core::instruction::InitConfig {}.data(),
            }],
            &[&admin],
            &admin.pubkey(),
        )
        .unwrap();
        w
    }

    fn send(
        &mut self,
        ixs: &[Instruction],
        signers: &[&Keypair],
        payer: &Pubkey,
    ) -> Result<(), String> {
        let mut msg = Message::new(ixs, Some(payer));
        msg.recent_blockhash = self.svm.latest_blockhash();
        // Missing-signer negative tests must surface as Err, not a panic.
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers)
            .map_err(|e| format!("signing: {e:?}"))?;
        let result = self
            .svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("execution: {:?} logs: {:#?}", e.err, e.meta.logs));
        self.svm.expire_blockhash();
        result
    }

    fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }

    fn warp_days(&mut self, days: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp += days * 86_400;
        self.svm.set_sysvar::<Clock>(&clock);
    }

    fn open_shop(&mut self, name: &str) -> Shop {
        let authority = Keypair::new();
        self.svm
            .airdrop(&authority.pubkey(), 50_000_000_000)
            .unwrap();
        let merchant = Pubkey::find_program_address(
            &[MERCHANT_SEED, authority.pubkey().as_ref(), &0u64.to_le_bytes()],
            &vesta_core::id(),
        )
        .0;
        let mint =
            Pubkey::find_program_address(&[MINT_SEED, merchant.as_ref()], &vesta_core::id()).0;
        let treasury = get_associated_token_address_with_program_id(
            &authority.pubkey(),
            &mint,
            &TOKEN_2022_ID,
        );
        self.send(
            &[Instruction {
                program_id: vesta_core::id(),
                accounts: vesta_core::accounts::RegisterMerchant {
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
                data: vesta_core::instruction::RegisterMerchant { id: 0,
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
            }],
            &[&authority],
            &authority.pubkey(),
        )
        .unwrap();
        Shop {
            authority,
            merchant,
            mint,
            treasury,
        }
    }

    fn earn(&mut self, shop: &Shop, customer: Pubkey, amount_base: u64) {
        let visit_day = (self.now() / 86_400) as u32;
        let profile = Pubkey::find_program_address(
            &[CUSTOMER_SEED, shop.merchant.as_ref(), customer.as_ref()],
            &vesta_core::id(),
        )
        .0;
        let ata =
            get_associated_token_address_with_program_id(&customer, &shop.mint, &TOKEN_2022_ID);
        let authority = shop.authority.insecure_clone();
        let ix = Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::EarnPoints {
                merchant_authority: authority.pubkey(),
                merchant: shop.merchant,
                customer,
                customer_profile: profile,
                point_mint: shop.mint,
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

    fn balance(&self, mint: Pubkey, owner: Pubkey) -> u64 {
        let ata = get_associated_token_address_with_program_id(&owner, &mint, &TOKEN_2022_ID);
        match self.svm.get_account(&ata) {
            Some(acc) if !acc.data.is_empty() => {
                StateWithExtensions::<TokenAccount>::unpack(&acc.data)
                    .unwrap()
                    .base
                    .amount
            }
            _ => 0,
        }
    }

    fn alliance_pda(&self, creator: Pubkey, id: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[ALLIANCE_SEED, creator.as_ref(), &id.to_le_bytes()],
            &vesta_core::id(),
        )
        .0
    }

    fn member_pda(&self, alliance: Pubkey, merchant: Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[MEMBER_SEED, alliance.as_ref(), merchant.as_ref()],
            &vesta_core::id(),
        )
        .0
    }

    fn create_alliance(&mut self, creator: &Keypair, id: u64) -> Pubkey {
        let alliance = self.alliance_pda(creator.pubkey(), id);
        let ix = Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::CreateAlliance {
                creator: creator.pubkey(),
                alliance,
                config: self.config,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::CreateAlliance {
                id,
                name: "Koinon".into(),
            }
            .data(),
        };
        self.send(&[ix], &[creator], &creator.pubkey()).unwrap();
        alliance
    }

    fn join_ix(
        &self,
        shop: &Shop,
        alliance: Pubkey,
        alliance_authority: Pubkey,
        rate: u32,
        budget: u64,
    ) -> Instruction {
        Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::JoinAlliance {
                merchant_authority: shop.authority.pubkey(),
                alliance_authority,
                merchant: shop.merchant,
                alliance,
                member: self.member_pda(alliance, shop.merchant),
                config: self.config,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::JoinAlliance {
                rate_bps_to_alliance: rate,
                swap_in_budget_raw: budget,
            }
            .data(),
        }
    }

    fn swap_ix(
        &self,
        customer: Pubkey,
        alliance: Pubkey,
        from: &Shop,
        to: &Shop,
        ui_amount: u64,
        max_raw_in: u64,
        min_raw_out: u64,
    ) -> Instruction {
        Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::SwapPoints {
                customer,
                alliance,
                member_a: self.member_pda(alliance, from.merchant),
                member_b: self.member_pda(alliance, to.merchant),
                merchant_a: from.merchant,
                merchant_b: to.merchant,
                mint_a: from.mint,
                mint_b: to.mint,
                customer_ata_a: get_associated_token_address_with_program_id(
                    &customer,
                    &from.mint,
                    &TOKEN_2022_ID,
                ),
                customer_ata_b: get_associated_token_address_with_program_id(
                    &customer,
                    &to.mint,
                    &TOKEN_2022_ID,
                ),
                config: self.config,
                token_program: TOKEN_2022_ID,
                associated_token_program: ATA_PROGRAM_ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::SwapPoints {
                ui_amount,
                max_raw_in,
                min_raw_out,
            }
            .data(),
        }
    }

    fn member_state(&self, alliance: Pubkey, merchant: Pubkey) -> AllianceMember {
        let data = self
            .svm
            .get_account(&self.member_pda(alliance, merchant))
            .unwrap()
            .data;
        AllianceMember::try_deserialize(&mut data.as_slice()).unwrap()
    }

    fn set_alliance_params(
        &mut self,
        authority: &Keypair,
        alliance: Pubkey,
        fee_bps: u16,
        min_rate_bps: u32,
        max_rate_bps: u32,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::AllianceAuthorityOnly {
                authority: authority.pubkey(),
                alliance,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::SetAllianceParams {
                fee_bps,
                min_rate_bps,
                max_rate_bps,
            }
            .data(),
        };
        self.send(&[ix], &[authority], &authority.pubkey())
    }

    fn set_alliance_paused(
        &mut self,
        authority: &Keypair,
        alliance: Pubkey,
        paused: bool,
    ) -> Result<(), String> {
        let ix = Instruction {
            program_id: vesta_core::id(),
            accounts: vesta_core::accounts::AllianceAuthorityOnly {
                authority: authority.pubkey(),
                alliance,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::SetAlliancePaused { paused }.data(),
        };
        self.send(&[ix], &[authority], &authority.pubkey())
    }
}

#[test]
fn alliance_handshake_and_swap_with_budget() {
    let mut w = World::new();
    let kavarna = w.open_shop("Kavarna");
    let litera = w.open_shop("Litera");
    let customer = Keypair::new();
    w.svm.airdrop(&customer.pubkey(), 10_000_000_000).unwrap();

    let creator = kavarna.authority.insecure_clone();
    let alliance = w.create_alliance(&creator, 1);

    // Join requires the alliance-authority co-signature.
    let join_litera = w.join_ix(&litera, alliance, creator.pubkey(), 10_000, 25_000);
    let litera_auth = litera.authority.insecure_clone();
    assert!(
        w.send(
            std::slice::from_ref(&join_litera),
            &[&litera_auth],
            &litera_auth.pubkey()
        )
        .is_err(),
        "join without alliance authority accepted"
    );
    w.send(
        &[join_litera],
        &[&litera_auth, &creator],
        &litera_auth.pubkey(),
    )
    .unwrap();

    let join_kavarna = w.join_ix(&kavarna, alliance, creator.pubkey(), 10_000, 25_000);
    w.send(&[join_kavarna], &[&creator], &creator.pubkey())
        .unwrap();

    // Double join → member PDA exists.
    let rejoin = w.join_ix(&kavarna, alliance, creator.pubkey(), 10_000, 25_000);
    assert!(w.send(&[rejoin], &[&creator], &creator.pubkey()).is_err());

    // Earn on Kavarna, swap 100.00 UI pts into Litera points.
    w.earn(&kavarna, customer.pubkey(), 5_000); // 510_000 raw
    let before_a = w.balance(kavarna.mint, customer.pubkey());
    let swap = w.swap_ix(
        customer.pubkey(),
        alliance,
        &kavarna,
        &litera,
        10_000,
        11_000,
        9_000,
    );
    w.send(
        &[cu_limit_ix(400_000), swap],
        &[&customer],
        &customer.pubkey(),
    )
    .unwrap();

    let raw_in = before_a - w.balance(kavarna.mint, customer.pubkey());
    let raw_out = w.balance(litera.mint, customer.pubkey());
    assert!((9_900..=10_100).contains(&raw_in), "raw_in {raw_in}");
    assert!((9_900..=10_100).contains(&raw_out), "raw_out {raw_out}");

    let member_b = w.member_state(alliance, litera.merchant);
    assert_eq!(member_b.swapped_in_today, raw_out);

    // Budget boundary: another 100.00 UI would exceed Litera's 25_000 budget
    // only on the third swap; second passes, third fails.
    let swap = w.swap_ix(
        customer.pubkey(),
        alliance,
        &kavarna,
        &litera,
        10_000,
        11_000,
        9_000,
    );
    w.send(
        &[cu_limit_ix(400_000), swap],
        &[&customer],
        &customer.pubkey(),
    )
    .unwrap();
    let swap = w.swap_ix(
        customer.pubkey(),
        alliance,
        &kavarna,
        &litera,
        10_000,
        11_000,
        9_000,
    );
    assert!(
        w.send(
            &[cu_limit_ix(400_000), swap],
            &[&customer],
            &customer.pubkey()
        )
        .is_err(),
        "swap-in budget not enforced"
    );

    // Day rollover resets the budget window.
    w.warp_days(1);
    let swap = w.swap_ix(
        customer.pubkey(),
        alliance,
        &kavarna,
        &litera,
        10_000,
        11_000,
        9_000,
    );
    w.send(
        &[cu_limit_ix(400_000), swap],
        &[&customer],
        &customer.pubkey(),
    )
    .unwrap();

    // Slippage: absurd min_raw_out fails.
    let swap = w.swap_ix(
        customer.pubkey(),
        alliance,
        &kavarna,
        &litera,
        1_000,
        2_000,
        5_000,
    );
    assert!(
        w.send(
            &[cu_limit_ix(400_000), swap],
            &[&customer],
            &customer.pubkey()
        )
        .is_err(),
        "min_raw_out ignored"
    );
}

#[test]
fn ui_denominated_swap_is_fair_across_mint_ages() {
    let mut w = World::new();
    let old_shop = w.open_shop("Ancient");
    let customer = Keypair::new();
    w.svm.airdrop(&customer.pubkey(), 10_000_000_000).unwrap();
    w.earn(&old_shop, customer.pubkey(), 5_000);

    // A full year of decay passes before the second mint is born.
    w.warp_days(365);
    let new_shop = w.open_shop("Fresh");

    let creator = old_shop.authority.insecure_clone();
    let alliance = w.create_alliance(&creator, 1);
    let join_old = w.join_ix(&old_shop, alliance, creator.pubkey(), 10_000, u64::MAX);
    w.send(&[join_old], &[&creator], &creator.pubkey()).unwrap();
    let join_new = w.join_ix(&new_shop, alliance, creator.pubkey(), 10_000, u64::MAX);
    let new_auth = new_shop.authority.insecure_clone();
    w.send(&[join_new], &[&new_auth, &creator], &new_auth.pubkey())
        .unwrap();

    // Equal rates, equal UI value — but the year-old mint needs ~e^0.2 more
    // raw per UI point. Raw-denominated swaps would have been an arbitrage
    // faucet; UI-denominated legs stay fair.
    let before_a = w.balance(old_shop.mint, customer.pubkey());
    let swap = w.swap_ix(
        customer.pubkey(),
        alliance,
        &old_shop,
        &new_shop,
        10_000,
        13_500,
        9_000,
    );
    w.send(
        &[cu_limit_ix(400_000), swap],
        &[&customer],
        &customer.pubkey(),
    )
    .unwrap();

    let raw_in = before_a - w.balance(old_shop.mint, customer.pubkey());
    let raw_out = w.balance(new_shop.mint, customer.pubkey());
    assert!(
        raw_in > raw_out + 1_500,
        "mint-age fairness broken: raw_in {raw_in} vs raw_out {raw_out}"
    );
    assert!(
        (11_800..=12_600).contains(&raw_in),
        "expected ~e^0.2 scaling, got {raw_in}"
    );
    assert!(
        (9_900..=10_100).contains(&raw_out),
        "fresh mint raw_out {raw_out}"
    );
}

#[test]
fn leave_alliance_blocks_swaps_and_requires_rehandshake() {
    let mut w = World::new();
    let kavarna = w.open_shop("Kavarna");
    let litera = w.open_shop("Litera");
    let customer = Keypair::new();
    w.svm.airdrop(&customer.pubkey(), 10_000_000_000).unwrap();
    w.earn(&kavarna, customer.pubkey(), 5_000);

    let creator = kavarna.authority.insecure_clone();
    let alliance = w.create_alliance(&creator, 1);
    let join_k = w.join_ix(&kavarna, alliance, creator.pubkey(), 10_000, u64::MAX);
    w.send(&[join_k], &[&creator], &creator.pubkey()).unwrap();
    let join_l = w.join_ix(&litera, alliance, creator.pubkey(), 10_000, u64::MAX);
    let litera_auth = litera.authority.insecure_clone();
    w.send(&[join_l], &[&litera_auth, &creator], &litera_auth.pubkey())
        .unwrap();

    // Litera leaves.
    let leave = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::LeaveAlliance {
            merchant_authority: litera_auth.pubkey(),
            merchant: litera.merchant,
            alliance,
            member: w.member_pda(alliance, litera.merchant),
        }
        .to_account_metas(None),
        data: vesta_core::instruction::LeaveAlliance {}.data(),
    };
    w.send(&[leave], &[&litera_auth], &litera_auth.pubkey())
        .unwrap();

    let alliance_data = w.svm.get_account(&alliance).unwrap().data;
    let a = Alliance::try_deserialize(&mut alliance_data.as_slice()).unwrap();
    assert_eq!(a.member_count, 1);
    assert!(w
        .svm
        .get_account(&w.member_pda(alliance, litera.merchant))
        .is_none());
    let merchant_data = w.svm.get_account(&litera.merchant).unwrap().data;
    let m = Merchant::try_deserialize(&mut merchant_data.as_slice()).unwrap();
    assert_eq!(m.joined_alliance, None);

    // Swaps against the departed member fail.
    let swap = w.swap_ix(
        customer.pubkey(),
        alliance,
        &kavarna,
        &litera,
        1_000,
        2_000,
        0,
    );
    assert!(w
        .send(
            &[cu_limit_ix(400_000), swap],
            &[&customer],
            &customer.pubkey()
        )
        .is_err());

    // Re-join still needs the handshake.
    let rejoin = w.join_ix(&litera, alliance, creator.pubkey(), 10_000, u64::MAX);
    assert!(
        w.send(
            std::slice::from_ref(&rejoin),
            &[&litera_auth],
            &litera_auth.pubkey()
        )
        .is_err(),
        "re-join without handshake accepted"
    );
    w.send(&[rejoin], &[&litera_auth, &creator], &litera_auth.pubkey())
        .unwrap();
}

#[test]
fn alliance_authority_two_step_and_rate_cosign() {
    let mut w = World::new();
    let kavarna = w.open_shop("Kavarna");
    let creator = kavarna.authority.insecure_clone();
    let alliance = w.create_alliance(&creator, 1);
    let join = w.join_ix(&kavarna, alliance, creator.pubkey(), 10_000, 1_000);
    w.send(&[join], &[&creator], &creator.pubkey()).unwrap();

    // Rate change without the alliance authority co-signature fails.
    let member = w.member_pda(alliance, kavarna.merchant);
    let rate_ix = |alliance_authority: Pubkey| Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::SetSwapRate {
            merchant_authority: creator.pubkey(),
            alliance_authority,
            merchant: kavarna.merchant,
            alliance,
            member,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::SetSwapRate { new_rate: 12_000 }.data(),
    };
    let rando = Keypair::new();
    w.svm.airdrop(&rando.pubkey(), 1_000_000_000).unwrap();
    assert!(
        w.send(
            &[rate_ix(rando.pubkey())],
            &[&creator, &rando],
            &creator.pubkey()
        )
        .is_err(),
        "rate change with wrong co-signer accepted"
    );
    w.send(&[rate_ix(creator.pubkey())], &[&creator], &creator.pubkey())
        .unwrap();
    assert_eq!(
        w.member_state(alliance, kavarna.merchant)
            .rate_bps_to_alliance,
        12_000
    );

    // Budget change is member-only.
    let budget_ix = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::SetSwapBudget {
            merchant_authority: creator.pubkey(),
            merchant: kavarna.merchant,
            member,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::SetSwapBudget { new_budget: 777 }.data(),
    };
    w.send(&[budget_ix], &[&creator], &creator.pubkey())
        .unwrap();
    assert_eq!(
        w.member_state(alliance, kavarna.merchant)
            .swap_in_budget_raw,
        777
    );

    // Two-step authority rotation.
    let new_authority = Keypair::new();
    w.svm
        .airdrop(&new_authority.pubkey(), 1_000_000_000)
        .unwrap();
    let propose = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::AllianceAuthorityOnly {
            authority: creator.pubkey(),
            alliance,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::TransferAllianceAuthority {
            new_authority: new_authority.pubkey(),
        }
        .data(),
    };
    w.send(&[propose], &[&creator], &creator.pubkey()).unwrap();

    let accept = |signer: &Keypair| Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::AcceptAllianceAuthority {
            pending_authority: signer.pubkey(),
            alliance,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::AcceptAllianceAuthority {}.data(),
    };
    assert!(w
        .send(&[accept(&rando)], &[&rando], &rando.pubkey())
        .is_err());
    w.send(
        &[accept(&new_authority)],
        &[&new_authority],
        &new_authority.pubkey(),
    )
    .unwrap();

    let alliance_data = w.svm.get_account(&alliance).unwrap().data;
    let a = Alliance::try_deserialize(&mut alliance_data.as_slice()).unwrap();
    assert_eq!(a.authority, new_authority.pubkey());
}

#[test]
fn clawback_is_hooked_audited_and_treasury_bound() {
    let mut w = World::new();
    let kavarna = w.open_shop("Kavarna");
    let customer = Keypair::new();
    w.svm.airdrop(&customer.pubkey(), 10_000_000_000).unwrap();
    w.earn(&kavarna, customer.pubkey(), 5_000); // 510_000 raw

    // Guard must be live for the hooked transfer to resolve.
    let authority = kavarna.authority.insecure_clone();
    let eaml = Pubkey::find_program_address(
        &[b"extra-account-metas", kavarna.mint.as_ref()],
        &argus::id(),
    )
    .0;
    let guard_config =
        Pubkey::find_program_address(&[b"guard", kavarna.mint.as_ref()], &argus::id()).0;
    let guard_init = Instruction {
        program_id: argus::id(),
        accounts: argus::accounts::InitializeTransferGuard {
            merchant_authority: authority.pubkey(),
            merchant: kavarna.merchant,
            mint: kavarna.mint,
            guard_config,
            extra_account_meta_list: eaml,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: argus::instruction::InitializeTransferGuard {
            policy: argus::instructions::policy::InitialPolicy {
                flags: argus::constants::flags::BLOCK_PROGRAM_OWNED,
                daily_gift_cap: argus::constants::DEFAULT_DAILY_GIFT_CAP_RAW,
                per_tx_cap: 0,
                max_wallet_balance: 0,
                transfers_per_day_cap: 0,
                cooldown_secs: 0,
                attestation_issuer: Pubkey::default(),
                attestation_schema: 0,
                attestation_mask: 0,
            },
        }
        .data(),
    };
    w.send(&[guard_init], &[&authority], &authority.pubkey())
        .unwrap();

    let customer_ata = get_associated_token_address_with_program_id(
        &customer.pubkey(),
        &kavarna.mint,
        &TOKEN_2022_ID,
    );
    let config = w.config;
    // argus hook extras for a clawback (source owner = customer, destination
    // owner = merchant authority who owns the treasury), then argus program +
    // eaml — passed as remaining_accounts in meta-list order.
    let g = |seeds: &[&[u8]]| Pubkey::find_program_address(seeds, &argus::id()).0;
    let clawback_ix = |amount: u64, destination: Pubkey, dest_owner: Pubkey, reason: u16| {
        let attestation = Pubkey::find_program_address(
            &[
                b"attestation",
                Pubkey::default().as_ref(),
                dest_owner.as_ref(),
            ],
            &aegis::id(),
        )
        .0;
        let customer_profile = Pubkey::find_program_address(
            &[CUSTOMER_SEED, kavarna.merchant.as_ref(), customer.pubkey().as_ref()],
            &vesta_core::id(),
        )
        .0;
        let mut accounts = vesta_core::accounts::ClawbackPoints {
            merchant_authority: authority.pubkey(),
            merchant: kavarna.merchant,
            customer: customer.pubkey(),
            customer_profile,
            customer_ata,
            treasury: destination,
            point_mint: kavarna.mint,
            config,
            token_program: TOKEN_2022_ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None);
        accounts.extend([
            AccountMeta::new_readonly(guard_config, false),
            AccountMeta::new(
                g(&[b"wstate", kavarna.mint.as_ref(), customer.pubkey().as_ref()]),
                false,
            ),
            AccountMeta::new_readonly(dest_owner, false),
            AccountMeta::new_readonly(g(&[b"entry", kavarna.mint.as_ref(), dest_owner.as_ref()]), false),
            AccountMeta::new_readonly(aegis::id(), false),
            AccountMeta::new_readonly(Pubkey::default(), false),
            AccountMeta::new_readonly(attestation, false),
            AccountMeta::new_readonly(argus::id(), false),
            AccountMeta::new_readonly(eaml, false),
        ]);
        Instruction {
            program_id: vesta_core::id(),
            accounts,
            data: vesta_core::instruction::Clawback {
                amount_raw: amount,
                reason_code: reason,
            }
            .data(),
        }
    };

    // A zero reason code is rejected (compliance: every clawback cites a reason).
    assert!(
        w.send(
            &[clawback_ix(1_000, kavarna.treasury, authority.pubkey(), 0)],
            &[&authority],
            &authority.pubkey()
        )
        .is_err(),
        "reasonless clawback accepted"
    );

    // Works with NO gift ledger opened (argus rule 1 short-circuits).
    let before = w.balance(kavarna.mint, customer.pubkey());
    w.send(
        &[clawback_ix(100_000, kavarna.treasury, authority.pubkey(), 7)],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    assert_eq!(w.balance(kavarna.mint, customer.pubkey()), before - 100_000);
    let treasury_data = w.svm.get_account(&kavarna.treasury).unwrap().data;
    let treasury_state = StateWithExtensions::<TokenAccount>::unpack(&treasury_data).unwrap();
    assert_eq!(treasury_state.base.amount, 100_000);

    // Tracking: merchant + per-customer counters recorded.
    let m_data = w.svm.get_account(&kavarna.merchant).unwrap().data;
    let m = Merchant::try_deserialize(&mut m_data.as_slice()).unwrap();
    assert_eq!(m.clawback_count, 1);
    assert_eq!(m.lifetime_clawed_back, 100_000);
    assert_eq!(m.clawed_today, 100_000);
    let profile_pda = Pubkey::find_program_address(
        &[CUSTOMER_SEED, kavarna.merchant.as_ref(), customer.pubkey().as_ref()],
        &vesta_core::id(),
    )
    .0;
    let p_data = w.svm.get_account(&profile_pda).unwrap().data;
    let p = CustomerProfile::try_deserialize(&mut p_data.as_slice()).unwrap();
    assert_eq!(p.clawback_count, 1);
    assert_eq!(p.lifetime_clawed_back, 100_000);

    // Destination other than the merchant treasury is rejected by has_one.
    let elsewhere = get_associated_token_address_with_program_id(
        &customer.pubkey(),
        &kavarna.mint,
        &TOKEN_2022_ID,
    );
    assert!(
        w.send(
            &[clawback_ix(1_000, elsewhere, customer.pubkey(), 7)],
            &[&authority],
            &authority.pubkey()
        )
        .is_err(),
        "clawback to non-treasury accepted"
    );

    // Cannot claw more than the balance.
    assert!(
        w.send(
            &[clawback_ix(10_000_000, kavarna.treasury, authority.pubkey(), 7)],
            &[&authority],
            &authority.pubkey()
        )
        .is_err(),
        "over-balance clawback accepted"
    );

    // Daily cap: set to 150_000 (100_000 already clawed today).
    let set_cap = Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::MerchantOwnerOnly {
            authority: authority.pubkey(),
            merchant: kavarna.merchant,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::SetClawbackCap {
            daily_cap_raw: 150_000,
        }
        .data(),
    };
    w.send(&[set_cap], &[&authority], &authority.pubkey()).unwrap();
    // +40_000 → 140_000 ≤ 150_000 passes.
    w.send(
        &[clawback_ix(40_000, kavarna.treasury, authority.pubkey(), 7)],
        &[&authority],
        &authority.pubkey(),
    )
    .unwrap();
    // +40_000 → 180_000 > 150_000 rejected.
    assert!(
        w.send(
            &[clawback_ix(40_000, kavarna.treasury, authority.pubkey(), 7)],
            &[&authority],
            &authority.pubkey()
        )
        .is_err(),
        "daily cap not enforced"
    );
}

#[test]
fn alliance_governance_bounds_and_pause() {
    let mut w = World::new();
    let kavarna = w.open_shop("Kavarna");
    let litera = w.open_shop("Litera");
    let customer = Keypair::new();
    w.svm.airdrop(&customer.pubkey(), 10_000_000_000).unwrap();
    let creator = kavarna.authority.insecure_clone();
    let litera_auth = litera.authority.insecure_clone();
    let alliance = w.create_alliance(&creator, 2);

    // Governance: member rates must be within [5_000, 15_000].
    w.set_alliance_params(&creator, alliance, 0, 5_000, 15_000).unwrap();
    // Non-authority cannot set params.
    assert!(
        w.set_alliance_params(&litera_auth, alliance, 0, 1, 2).is_err(),
        "non-authority set alliance params"
    );

    // Join below the min rate → rejected; in-bounds → accepted.
    let bad = w.join_ix(&litera, alliance, creator.pubkey(), 3_000, 25_000);
    assert!(
        w.send(&[bad], &[&litera_auth, &creator], &litera_auth.pubkey())
            .is_err(),
        "out-of-bounds rate joined"
    );
    let join_litera = w.join_ix(&litera, alliance, creator.pubkey(), 10_000, 25_000);
    w.send(&[join_litera], &[&litera_auth, &creator], &litera_auth.pubkey())
        .unwrap();
    let join_kavarna = w.join_ix(&kavarna, alliance, creator.pubkey(), 10_000, 25_000);
    w.send(&[join_kavarna], &[&creator], &creator.pubkey())
        .unwrap();

    // Pause the alliance → swaps are frozen.
    w.earn(&kavarna, customer.pubkey(), 5_000);
    w.set_alliance_paused(&creator, alliance, true).unwrap();
    let swap = w.swap_ix(customer.pubkey(), alliance, &kavarna, &litera, 10_000, 11_000, 9_000);
    assert!(
        w.send(&[cu_limit_ix(400_000), swap], &[&customer], &customer.pubkey())
            .is_err(),
        "paused alliance swapped"
    );

    // Resume → swap works and volume stats accrue.
    w.set_alliance_paused(&creator, alliance, false).unwrap();
    let swap = w.swap_ix(customer.pubkey(), alliance, &kavarna, &litera, 10_000, 11_000, 9_000);
    w.send(&[cu_limit_ix(400_000), swap], &[&customer], &customer.pubkey())
        .unwrap();
    assert!(w.balance(litera.mint, customer.pubkey()) > 0);
}

#[test]
fn alliance_can_suspend_a_member() {
    let mut w = World::new();
    let kavarna = w.open_shop("Kavarna");
    let litera = w.open_shop("Litera");
    let customer = Keypair::new();
    w.svm.airdrop(&customer.pubkey(), 10_000_000_000).unwrap();
    let creator = kavarna.authority.insecure_clone();
    let litera_auth = litera.authority.insecure_clone();
    let alliance = w.create_alliance(&creator, 3);

    let join_litera = w.join_ix(&litera, alliance, creator.pubkey(), 10_000, 25_000);
    w.send(&[join_litera], &[&litera_auth, &creator], &litera_auth.pubkey())
        .unwrap();
    let join_kavarna = w.join_ix(&kavarna, alliance, creator.pubkey(), 10_000, 25_000);
    w.send(&[join_kavarna], &[&creator], &creator.pubkey()).unwrap();
    w.earn(&kavarna, customer.pubkey(), 5_000);

    let litera_member = w.member_pda(alliance, litera.merchant);
    let set_active = |active: bool| Instruction {
        program_id: vesta_core::id(),
        accounts: vesta_core::accounts::SetMemberActive {
            authority: creator.pubkey(),
            alliance,
            member: litera_member,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::SetMemberActive { active }.data(),
    };

    // Suspend Litera → swaps into it are frozen.
    w.send(&[set_active(false)], &[&creator], &creator.pubkey()).unwrap();
    let swap = w.swap_ix(customer.pubkey(), alliance, &kavarna, &litera, 10_000, 11_000, 9_000);
    assert!(
        w.send(&[cu_limit_ix(400_000), swap], &[&customer], &customer.pubkey())
            .is_err(),
        "swap into a suspended member succeeded"
    );

    // Reactivate → swap works.
    w.send(&[set_active(true)], &[&creator], &creator.pubkey()).unwrap();
    let swap = w.swap_ix(customer.pubkey(), alliance, &kavarna, &litera, 10_000, 11_000, 9_000);
    w.send(&[cu_limit_ix(400_000), swap], &[&customer], &customer.pubkey())
        .unwrap();
    assert!(w.balance(litera.mint, customer.pubkey()) > 0);
}
