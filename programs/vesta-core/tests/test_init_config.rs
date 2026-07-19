use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    litesvm::LiteSVM,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

fn program_bytes() -> &'static [u8] {
    include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/vesta_core.so"
    ))
}

#[test]
fn init_config_sets_admin() {
    let program_id = vesta_core::id();
    let admin = Keypair::new();
    let config = Pubkey::find_program_address(&[vesta_core::constants::CONFIG_SEED], &program_id).0;

    let mut svm = LiteSVM::new();
    svm.add_program(program_id, program_bytes()).unwrap();
    svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();

    let ix = Instruction {
        program_id,
        accounts: vesta_core::accounts::InitConfig {
            admin: admin.pubkey(),
            config,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vesta_core::instruction::InitConfig {}.data(),
    };

    let mut msg = Message::new(&[ix], Some(&admin.pubkey()));
    msg.recent_blockhash = svm.latest_blockhash();
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&admin]).unwrap();
    svm.send_transaction(tx).unwrap();

    let account = svm.get_account(&config).unwrap();
    let state = vesta_core::state::Config::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(state.admin, admin.pubkey());
    assert!(!state.paused);
}

#[test]
fn init_config_twice_fails() {
    let program_id = vesta_core::id();
    let admin = Keypair::new();
    let config = Pubkey::find_program_address(&[vesta_core::constants::CONFIG_SEED], &program_id).0;

    let mut svm = LiteSVM::new();
    svm.add_program(program_id, program_bytes()).unwrap();
    svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();

    let build_tx = |svm: &LiteSVM| {
        let ix = Instruction {
            program_id,
            accounts: vesta_core::accounts::InitConfig {
                admin: admin.pubkey(),
                config,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vesta_core::instruction::InitConfig {}.data(),
        };
        let mut msg = Message::new(&[ix], Some(&admin.pubkey()));
        msg.recent_blockhash = svm.latest_blockhash();
        VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&admin]).unwrap()
    };

    svm.send_transaction(build_tx(&svm)).unwrap();
    svm.expire_blockhash();
    assert!(svm.send_transaction(build_tx(&svm)).is_err());
}
