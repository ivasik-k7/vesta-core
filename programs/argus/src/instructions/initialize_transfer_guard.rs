use anchor_lang::{
    prelude::*,
    system_program::{
        allocate, assign, create_account, transfer, Allocate, Assign, CreateAccount, Transfer,
    },
};
use spl_tlv_account_resolution::{
    account::ExtraAccountMeta, pubkey_data::PubkeyData, seeds::Seed, state::ExtraAccountMetaList,
};
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

use crate::{
    constants::{
        AEGIS_ID, ATTESTATION_SEED, EXTRA_ACCOUNT_METAS_SEED, GUARD_SEED, LIST_ENTRY_SEED,
        WALLET_STATE_SEED,
    },
    error::GuardError,
    events::TransferGuardInitialized,
    instructions::policy::{validate_policy, InitialPolicy},
    state::GuardConfig,
};

#[derive(Accounts)]
pub struct InitializeTransferGuard<'info> {
    #[account(mut)]
    pub merchant_authority: Signer<'info>,

    /// CHECK: vesta_core's Merchant PDA, verified manually in the handler —
    /// owner program, discriminator, PDA derivation, and field bindings.
    pub merchant: UncheckedAccount<'info>,

    /// CHECK: only used as a PDA seed and for binding against merchant.point_mint.
    pub mint: UncheckedAccount<'info>,

    /// Per-mint policy account (spec §2.1).
    #[account(
        init,
        payer = merchant_authority,
        space = 8 + GuardConfig::INIT_SPACE,
        seeds = [GUARD_SEED, mint.key().as_ref()],
        bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,

    /// CHECK: the interface-defined ExtraAccountMetaList PDA; created and
    /// TLV-initialized in the handler (defensively — the address is predictable).
    #[account(mut, seeds = [EXTRA_ACCOUNT_METAS_SEED, mint.key().as_ref()], bump)]
    pub extra_account_meta_list: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_transfer_guard(
    ctx: Context<InitializeTransferGuard>,
    policy: InitialPolicy,
) -> Result<()> {
    // Authorization chain (spec §3.1): the Merchant account is owned by
    // vesta_core, carries the Merchant discriminator, re-derives as
    // ["merchant", authority] under vesta_core, the signer IS that authority,
    // and the mint matches merchant.point_mint. Layout (fields are fixed-size
    // and lead the account): disc(8) · authority(32) · point_mint(32) · treasury(32).
    let merchant_info = ctx.accounts.merchant.to_account_info();
    require_keys_eq!(
        *merchant_info.owner,
        crate::constants::VESTA_CORE_ID,
        GuardError::UnauthorizedGuardInit
    );
    let (merchant_authority, merchant_point_mint, merchant_treasury) = {
        let data = merchant_info.try_borrow_data()?;
        require!(data.len() >= 104, GuardError::UnauthorizedGuardInit);
        require!(
            data[..8] == crate::constants::MERCHANT_DISCRIMINATOR,
            GuardError::UnauthorizedGuardInit
        );
        let key = |range: core::ops::Range<usize>| {
            Pubkey::try_from(&data[range]).map_err(|_| GuardError::UnauthorizedGuardInit)
        };
        (key(8..40)?, key(40..72)?, key(72..104)?)
    };
    require_keys_eq!(
        merchant_authority,
        ctx.accounts.merchant_authority.key(),
        GuardError::UnauthorizedGuardInit
    );
    let expected_merchant = Pubkey::find_program_address(
        &[b"merchant", merchant_authority.as_ref()],
        &crate::constants::VESTA_CORE_ID,
    )
    .0;
    require_keys_eq!(
        ctx.accounts.merchant.key(),
        expected_merchant,
        GuardError::UnauthorizedGuardInit
    );
    require_keys_eq!(
        merchant_point_mint,
        ctx.accounts.mint.key(),
        GuardError::MintMismatch
    );

    validate_policy(
        policy.flags,
        policy.daily_gift_cap,
        policy.per_tx_cap,
        policy.attestation_issuer,
    )?;

    let mint_key = ctx.accounts.mint.key();

    // Persist the policy.
    let config = &mut ctx.accounts.guard_config;
    config.mint = mint_key;
    config.authority = ctx.accounts.merchant_authority.key();
    config.pending_authority = None;
    config.treasury = merchant_treasury;
    config.attestation_issuer = policy.attestation_issuer;
    config.paused = false;
    config.flags = policy.flags;
    config.daily_gift_cap = policy.daily_gift_cap;
    config.per_tx_cap = policy.per_tx_cap;
    config.max_wallet_balance = policy.max_wallet_balance;
    config.transfers_per_day_cap = policy.transfers_per_day_cap;
    config.cooldown_secs = policy.cooldown_secs;
    config.attestation_schema = policy.attestation_schema;
    config.attestation_mask = policy.attestation_mask;
    config.bump = ctx.bumps.guard_config;

    let eaml_info = ctx.accounts.extra_account_meta_list.to_account_info();
    require_keys_eq!(
        *eaml_info.owner,
        anchor_lang::system_program::ID,
        GuardError::GuardAlreadyInitialized
    );

    // Execute account order (spec §5): 0 source · 1 mint · 2 destination ·
    // 3 authority · 4 meta-list · then these extras, indices 5..12.
    let metas = [
        // 5 GuardConfig — self PDA ["guard", mint], read.
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: GUARD_SEED.to_vec(),
                },
                Seed::AccountKey { index: 1 },
            ],
            false,
            false,
        )
        .map_err(|_| GuardError::MetaListMismatch)?,
        // 6 WalletPolicyState — ["wstate", mint, source token account owner
        // (account 0, offset 32)]; writable, delegation-proof by construction.
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: WALLET_STATE_SEED.to_vec(),
                },
                Seed::AccountKey { index: 1 },
                Seed::AccountData {
                    account_index: 0,
                    data_index: 32,
                    length: 32,
                },
            ],
            false,
            true,
        )
        .map_err(|_| GuardError::MetaListMismatch)?,
        // 7 destination owner wallet, dereferenced from the destination token
        // account's owner field — lets the hook inspect its owning program.
        ExtraAccountMeta::new_with_pubkey_data(
            &PubkeyData::AccountData {
                account_index: 2,
                data_index: 32,
            },
            false,
            false,
        )
        .map_err(|_| GuardError::MetaListMismatch)?,
        // 8 list entry — self PDA ["entry", mint, destination owner], read.
        // Existence == membership; used only when a list flag is set.
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: LIST_ENTRY_SEED.to_vec(),
                },
                Seed::AccountKey { index: 1 },
                Seed::AccountData {
                    account_index: 2,
                    data_index: 32,
                    length: 32,
                },
            ],
            false,
            false,
        )
        .map_err(|_| GuardError::MetaListMismatch)?,
        // 9 aegis program id, pinned as a literal so the attestation meta can
        // derive a PDA under it (spec §7).
        ExtraAccountMeta::new_with_pubkey(&AEGIS_ID, false, false)
            .map_err(|_| GuardError::MetaListMismatch)?,
        // 10 aegis issuer this guard trusts, pinned at init (immutable).
        ExtraAccountMeta::new_with_pubkey(&policy.attestation_issuer, false, false)
            .map_err(|_| GuardError::MetaListMismatch)?,
        // 11 attestation — external PDA under aegis (account 9):
        // ["attestation", issuer (account 10), destination owner], read.
        ExtraAccountMeta::new_external_pda_with_seeds(
            9,
            &[
                Seed::Literal {
                    bytes: ATTESTATION_SEED.to_vec(),
                },
                Seed::AccountKey { index: 10 },
                Seed::AccountData {
                    account_index: 2,
                    data_index: 32,
                    length: 32,
                },
            ],
            false,
            false,
        )
        .map_err(|_| GuardError::MetaListMismatch)?,
    ];

    let space = ExtraAccountMetaList::size_of(metas.len()).map_err(|_| GuardError::Overflow)?;
    let rent_target = Rent::get()?.minimum_balance(space);
    let eaml_seeds: &[&[u8]] = &[
        EXTRA_ACCOUNT_METAS_SEED,
        mint_key.as_ref(),
        &[ctx.bumps.extra_account_meta_list],
    ];

    // Defensive creation — a 1-lamport donation to the predictable address
    // must not brick guard initialization.
    if eaml_info.lamports() == 0 {
        create_account(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                CreateAccount {
                    from: ctx.accounts.merchant_authority.to_account_info(),
                    to: eaml_info.clone(),
                },
                &[eaml_seeds],
            ),
            rent_target,
            space as u64,
            &crate::ID,
        )?;
    } else {
        let top_up = rent_target.saturating_sub(eaml_info.lamports());
        if top_up > 0 {
            transfer(
                CpiContext::new(
                    ctx.accounts.system_program.key(),
                    Transfer {
                        from: ctx.accounts.merchant_authority.to_account_info(),
                        to: eaml_info.clone(),
                    },
                ),
                top_up,
            )?;
        }
        allocate(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                Allocate {
                    account_to_allocate: eaml_info.clone(),
                },
                &[eaml_seeds],
            ),
            space as u64,
        )?;
        assign(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                Assign {
                    account_to_assign: eaml_info.clone(),
                },
                &[eaml_seeds],
            ),
            &crate::ID,
        )?;
    }

    let mut data = eaml_info.try_borrow_mut_data()?;
    ExtraAccountMetaList::init::<ExecuteInstruction>(&mut data, &metas)
        .map_err(|_| GuardError::MetaListMismatch)?;

    emit!(TransferGuardInitialized {
        mint: mint_key,
        merchant: ctx.accounts.merchant.key(),
        authority: ctx.accounts.merchant_authority.key(),
    });
    Ok(())
}
