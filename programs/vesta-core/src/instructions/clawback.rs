use anchor_lang::{
    prelude::*,
    solana_program::{instruction::AccountMeta, program::invoke_signed},
};
use anchor_spl::token_2022::Token2022;

use crate::{
    constants::{CONFIG_SEED, DECIMALS, MERCHANT_SEED, MINT_SEED},
    error::VestaError,
    events::Clawback as ClawbackEvent,
    state::{Config, Merchant},
};

/// Issuer clawback via PermanentDelegate — implemented exclusively as
/// transfer_checked so argus observes and audits every one (spec §3.7).
/// The permanent delegate's burn capability is never exercised.
///
/// The argus transfer-hook extras (resolved from the ExtraAccountMetaList by
/// the caller), the hook program, and the meta list itself are passed as
/// `remaining_accounts` in interface order, so this instruction stays agnostic
/// to the guard's policy shape as argus evolves.
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

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    pub token_program: Program<'info, Token2022>,
    // remaining_accounts: argus hook extras (meta-list order), then the argus
    // program, then the ExtraAccountMetaList — exactly what Token-2022 expects
    // appended to a hooked transfer_checked.
}

pub fn handle_clawback<'info>(
    ctx: Context<'info, ClawbackPoints<'info>>,
    amount_raw: u64,
    reason_code: u16,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(amount_raw > 0, VestaError::InvalidAmount);

    // Base hooked transfer: customer ATA → treasury, signed by the merchant PDA
    // acting as the mint's permanent delegate.
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

    // Append the caller-resolved hook extras verbatim, preserving writability.
    for acc in ctx.remaining_accounts {
        ix.accounts.push(AccountMeta {
            pubkey: acc.key(),
            is_signer: false,
            is_writable: acc.is_writable,
        });
    }

    let mut infos = vec![
        ctx.accounts.customer_ata.to_account_info(),
        ctx.accounts.point_mint.to_account_info(),
        ctx.accounts.treasury.to_account_info(),
        ctx.accounts.merchant.to_account_info(),
    ];
    infos.extend(ctx.remaining_accounts.iter().cloned());

    let authority_key = ctx.accounts.merchant.authority;
    let merchant_seeds: &[&[u8]] = &[
        MERCHANT_SEED,
        authority_key.as_ref(),
        &[ctx.accounts.merchant.bump],
    ];
    invoke_signed(&ix, &infos, &[merchant_seeds])?;

    emit!(ClawbackEvent {
        merchant: ctx.accounts.merchant.key(),
        customer: ctx.accounts.customer.key(),
        amount_raw,
        reason_code,
    });
    Ok(())
}
