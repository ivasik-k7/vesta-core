use anchor_lang::prelude::*;
use anchor_spl::{
    token_2022::{set_authority, SetAuthority, Token2022},
    token_interface::Mint,
};
use spl_token_2022_interface::{
    extension::{transfer_hook::TransferHook, BaseStateWithExtensions, StateWithExtensions},
    instruction::AuthorityType,
};

use crate::{
    constants::{CONFIG_SEED, MERCHANT_SEED, MINT_SEED},
    error::VestaError,
    events::TransferGuardFinalized,
    instructions::register_merchant::ARGUS_ID,
    state::{Config, Merchant},
};

#[derive(Accounts)]
pub struct FinalizeTransferGuard<'info> {
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        mut,
        seeds = [MINT_SEED, merchant.key().as_ref()],
        bump = merchant.mint_bump,
    )]
    pub point_mint: InterfaceAccount<'info, Mint>,

    /// CHECK: argus's ExtraAccountMetaList for this mint — must exist and be
    /// owned by argus before the hook authority is burned (spec §3.2).
    pub extra_account_meta_list: UncheckedAccount<'info>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    pub token_program: Program<'info, Token2022>,
}

pub fn handle_finalize_transfer_guard(ctx: Context<FinalizeTransferGuard>) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);

    // Guard must be initialized: correct PDA under argus, owned by argus, non-empty.
    let expected_eaml = Pubkey::find_program_address(
        &[
            b"extra-account-metas",
            ctx.accounts.point_mint.key().as_ref(),
        ],
        &ARGUS_ID,
    )
    .0;
    let eaml = &ctx.accounts.extra_account_meta_list;
    require_keys_eq!(eaml.key(), expected_eaml, VestaError::GuardNotInitialized);
    require_keys_eq!(*eaml.owner, ARGUS_ID, VestaError::GuardNotInitialized);
    require!(!eaml.data_is_empty(), VestaError::GuardNotInitialized);

    // Idempotence guard: once the hook authority is None, this is done forever.
    {
        let mint_info = ctx.accounts.point_mint.to_account_info();
        let mint_data = mint_info.try_borrow_data()?;
        let state =
            StateWithExtensions::<spl_token_2022_interface::state::Mint>::unpack(&mint_data)
                .map_err(|_| VestaError::MintMismatch)?;
        let hook = state
            .get_extension::<TransferHook>()
            .map_err(|_| VestaError::GuardNotInitialized)?;
        require!(
            Option::<Pubkey>::from(hook.authority).is_some(),
            VestaError::GuardAlreadyFinalized
        );
    }

    let authority_key = ctx.accounts.merchant.authority;
    let merchant_seeds: &[&[u8]] = &[
        MERCHANT_SEED,
        authority_key.as_ref(),
        &[ctx.accounts.merchant.bump],
    ];
    set_authority(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            SetAuthority {
                current_authority: ctx.accounts.merchant.to_account_info(),
                account_or_mint: ctx.accounts.point_mint.to_account_info(),
            },
            &[merchant_seeds],
        ),
        AuthorityType::TransferHookProgramId,
        None,
    )?;

    emit!(TransferGuardFinalized {
        mint: ctx.accounts.point_mint.key(),
        merchant: ctx.accounts.merchant.key(),
    });
    Ok(())
}
