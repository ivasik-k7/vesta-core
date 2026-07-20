use anchor_lang::{
    prelude::*,
    solana_program::{instruction::AccountMeta, program::invoke_signed},
};
use anchor_spl::token_2022::Token2022;

use crate::{
    constants::{CONFIG_SEED, DECIMALS, MERCHANT_SEED, MINT_SEED},
    error::VestaError,
    events::Clawback as ClawbackEvent,
    instructions::register_merchant::ARGUS_ID,
    state::{Config, Merchant},
};

/// Issuer clawback via PermanentDelegate — implemented exclusively as
/// transfer_checked so argus observes and audits every one (spec §3.7).
/// The permanent delegate's burn capability is never exercised.
#[derive(Accounts)]
pub struct ClawbackPoints<'info> {
    pub merchant_authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, merchant_authority.key().as_ref()],
        bump = merchant.bump,
        has_one = treasury @ VestaError::TreasuryMismatch,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Account<'info, Merchant>,

    /// CHECK: identity only; the source ATA is derived from this wallet.
    pub customer: UncheckedAccount<'info>,

    /// CHECK: the customer's ATA for the point mint; validated by Token-2022
    /// during the transfer (owner/mint bindings).
    #[account(mut)]
    pub customer_ata: UncheckedAccount<'info>,

    /// CHECK: destination is pinned to merchant.treasury by the has_one above.
    #[account(mut)]
    pub treasury: UncheckedAccount<'info>,

    /// CHECK: the merchant point mint (PDA-bound below).
    #[account(
        mut,
        seeds = [MINT_SEED, merchant.key().as_ref()],
        bump = merchant.mint_bump,
    )]
    pub point_mint: UncheckedAccount<'info>,

    /// CHECK: argus ExtraAccountMetaList — required so the hooked transfer
    /// resolves (fail-closed otherwise).
    pub extra_account_meta_list: UncheckedAccount<'info>,

    /// CHECK: the customer's gift ledger slot; may not exist — argus rule 1
    /// short-circuits before deserializing it.
    #[account(mut)]
    pub gift_ledger: UncheckedAccount<'info>,

    /// CHECK: destination owner wallet (the merchant authority) — resolved
    /// extra for argus's pubkey-data meta.
    pub destination_owner: UncheckedAccount<'info>,

    /// CHECK: the argus program, invoked by Token-2022.
    #[account(address = ARGUS_ID)]
    pub argus_program: UncheckedAccount<'info>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    pub token_program: Program<'info, Token2022>,
}

pub fn handle_clawback(
    ctx: Context<ClawbackPoints>,
    amount_raw: u64,
    reason_code: u16,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(amount_raw > 0, VestaError::InvalidAmount);

    // Build the hooked transfer manually: base accounts + the argus extras in
    // meta-list order, then the hook program and the meta list itself.
    let mut ix = spl_token_2022_interface::instruction::transfer_checked(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.customer_ata.key(),
        &ctx.accounts.point_mint.key(),
        &ctx.accounts.treasury.key(),
        &ctx.accounts.merchant.key(), // permanent delegate = merchant PDA
        &[],
        amount_raw,
        DECIMALS,
    )
    .map_err(|_| VestaError::ConversionFailed)?;
    ix.accounts
        .push(AccountMeta::new(ctx.accounts.gift_ledger.key(), false));
    ix.accounts.push(AccountMeta::new_readonly(
        ctx.accounts.destination_owner.key(),
        false,
    ));
    ix.accounts.push(AccountMeta::new_readonly(
        ctx.accounts.treasury.key(),
        false,
    ));
    ix.accounts.push(AccountMeta::new_readonly(ARGUS_ID, false));
    ix.accounts.push(AccountMeta::new_readonly(
        ctx.accounts.extra_account_meta_list.key(),
        false,
    ));

    let authority_key = ctx.accounts.merchant.authority;
    let merchant_seeds: &[&[u8]] = &[
        MERCHANT_SEED,
        authority_key.as_ref(),
        &[ctx.accounts.merchant.bump],
    ];
    invoke_signed(
        &ix,
        &[
            ctx.accounts.customer_ata.to_account_info(),
            ctx.accounts.point_mint.to_account_info(),
            ctx.accounts.treasury.to_account_info(),
            ctx.accounts.merchant.to_account_info(),
            ctx.accounts.gift_ledger.to_account_info(),
            ctx.accounts.destination_owner.to_account_info(),
            ctx.accounts.argus_program.to_account_info(),
            ctx.accounts.extra_account_meta_list.to_account_info(),
        ],
        &[merchant_seeds],
    )?;

    emit!(ClawbackEvent {
        merchant: ctx.accounts.merchant.key(),
        customer: ctx.accounts.customer.key(),
        amount_raw,
        reason_code,
    });
    Ok(())
}
