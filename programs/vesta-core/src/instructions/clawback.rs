use anchor_lang::{
    prelude::*,
    solana_program::{instruction::AccountMeta, program::invoke_signed},
};
use anchor_spl::{token_2022::Token2022, token_interface::TokenAccount};

use crate::{
    constants::{CONFIG_SEED, CUSTOMER_SEED, DECIMALS, MERCHANT_SEED, MINT_SEED, SECONDS_PER_DAY},
    error::VestaError,
    events::Clawback as ClawbackEvent,
    state::{Config, CustomerProfile, Merchant},
};

/// Issuer clawback via PermanentDelegate — implemented exclusively as
/// transfer_checked so argus observes and audits every one (spec §3.7). The
/// permanent delegate's burn capability is never exercised.
///
/// Enterprise controls: authorized by the owner OR any merchant operator; a
/// non-zero reason code is mandatory; the amount is bounded to the customer's
/// balance; a per-merchant daily cap bounds a compromised key; and every
/// clawback updates merchant + per-customer counters and emits a full audit
/// record. The argus hook extras, the argus program, and the meta list are
/// passed as `remaining_accounts` in interface order.
#[derive(Accounts)]
pub struct ClawbackPoints<'info> {
    #[account(mut)]
    pub merchant_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant.authority.as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = treasury @ VestaError::TreasuryMismatch,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Account<'info, Merchant>,

    /// CHECK: identity only; the profile PDA and ATA ownership bind to this key.
    pub customer: UncheckedAccount<'info>,

    /// Per-customer clawback stats; created if the customer holds points via a
    /// swap without ever having earned at this merchant.
    #[account(
        init_if_needed,
        payer = merchant_authority,
        space = 8 + CustomerProfile::INIT_SPACE,
        seeds = [CUSTOMER_SEED, merchant.key().as_ref(), customer.key().as_ref()],
        bump,
    )]
    pub customer_profile: Account<'info, CustomerProfile>,

    /// The customer's point account — bound to the mint and the customer.
    #[account(
        mut,
        constraint = customer_ata.mint == point_mint.key() @ VestaError::MintMismatch,
        constraint = customer_ata.owner == customer.key() @ VestaError::Unauthorized,
    )]
    pub customer_ata: InterfaceAccount<'info, TokenAccount>,

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
    pub system_program: Program<'info, System>,
    // remaining_accounts: argus hook extras (meta-list order), then the argus
    // program, then the ExtraAccountMetaList.
}

pub fn handle_clawback<'info>(
    ctx: Context<'info, ClawbackPoints<'info>>,
    amount_raw: u64,
    reason_code: u16,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(amount_raw > 0, VestaError::InvalidAmount);
    // Compliance: every clawback must cite a reason.
    require!(reason_code != 0, VestaError::ReasonRequired);
    // Clawback confiscates a customer's balance — a high-privilege compliance
    // action. Owner-only, never an operator hot key: a compromised POS key must
    // not be able to drain the customer base (SECURITY_AUDIT M-1).
    require_keys_eq!(
        ctx.accounts.merchant_authority.key(),
        ctx.accounts.merchant.authority,
        VestaError::Unauthorized
    );

    // Bound to the customer's balance for a clean, accountable failure.
    let balance = ctx.accounts.customer_ata.amount;
    require!(amount_raw <= balance, VestaError::ClawbackExceedsBalance);
    let balance_after = balance.saturating_sub(amount_raw);

    // Per-merchant daily cap (0 = unlimited) — bounds a compromised key.
    let today = u32::try_from(Clock::get()?.unix_timestamp / SECONDS_PER_DAY)
        .map_err(|_| VestaError::Overflow)?;
    {
        let m = &mut ctx.accounts.merchant;
        if m.clawback_day != today {
            m.clawback_day = today;
            m.clawed_today = 0;
        }
        m.clawed_today = m
            .clawed_today
            .checked_add(amount_raw)
            .ok_or(VestaError::Overflow)?;
        if m.clawback_daily_cap_raw > 0 {
            require!(
                m.clawed_today <= m.clawback_daily_cap_raw,
                VestaError::ClawbackCapExceeded
            );
        }
    }

    // Base hooked transfer: customer ATA → treasury, signed by the merchant PDA
    // acting as the mint's permanent delegate.
    let mut ix = spl_token_2022_interface::instruction::transfer_checked(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.customer_ata.key(),
        &ctx.accounts.point_mint.key(),
        &ctx.accounts.treasury.key(),
        &ctx.accounts.merchant.key(),
        &[],
        amount_raw,
        DECIMALS,
    )
    .map_err(|_| VestaError::ConversionFailed)?;

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
    let id_bytes = ctx.accounts.merchant.id.to_le_bytes();
    let merchant_seeds: &[&[u8]] = &[
        MERCHANT_SEED,
        authority_key.as_ref(),
        &id_bytes,
        &[ctx.accounts.merchant.bump],
    ];
    invoke_signed(&ix, &infos, &[merchant_seeds])?;

    // Counters (merchant + per-customer). Fresh profile → wire identity.
    let merchant_key = ctx.accounts.merchant.key();
    let customer_key = ctx.accounts.customer.key();
    let first_touch = ctx.accounts.customer_profile.wallet == Pubkey::default();
    {
        let profile = &mut ctx.accounts.customer_profile;
        if first_touch {
            profile.wallet = customer_key;
            profile.merchant = merchant_key;
            profile.bump = ctx.bumps.customer_profile;
        }
        profile.lifetime_clawed_back = profile.lifetime_clawed_back.saturating_add(amount_raw);
    }
    if first_touch {
        ctx.accounts.merchant.customer_count =
            ctx.accounts.merchant.customer_count.saturating_add(1);
    }
    let profile = &mut ctx.accounts.customer_profile;
    profile.clawback_count = profile.clawback_count.saturating_add(1);

    let clawed_today = {
        let m = &mut ctx.accounts.merchant;
        m.lifetime_clawed_back = m
            .lifetime_clawed_back
            .saturating_add(u128::from(amount_raw));
        m.clawback_count = m.clawback_count.saturating_add(1);
        m.clawed_today
    };

    emit!(ClawbackEvent {
        merchant: merchant_key,
        customer: customer_key,
        actor: ctx.accounts.merchant_authority.key(),
        amount_raw,
        reason_code,
        balance_after,
        clawed_today,
    });
    Ok(())
}
