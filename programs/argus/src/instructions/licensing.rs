//! Multi-tenancy & licensing (spec 10, phase 5).
//!
//! argus is already a single program serving every mint from isolated,
//! mint-scoped PDAs — so multi-tenancy is structural, not bolted on. This module
//! adds the commercial layer: a protocol-wide fee treasury and per-mint premium
//! `LicenseState`. Premium features (governance / statements / trust / screening)
//! require a live, entitled license; the free tier (guard init, caps, velocity,
//! lists, refresh, execute) needs none, so a VESTA-style deployment onboards at
//! zero cost. Fees are charged on a licensing event (`purchase_license`), never
//! per transfer — holders are never taxed — and an expired license degrades a
//! mint to the free tier, never to a state that strands assets.

use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

use crate::{
    constants::{
        entitlement, GUARD_SEED, LICENSE_PERIOD_SECS, LICENSE_SEED, PROTOCOL_SEED, STATE_VERSION,
    },
    error::GuardError,
    events::{FeesWithdrawn, LicensePurchased, LicenseSet, ProtocolInitialized},
    state::{ArgusProtocol, GuardConfig, LicenseState},
};

// ── initialize_protocol (trust-on-first-use) ─────────────────────────────────

#[derive(Accounts)]
pub struct InitializeProtocol<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + ArgusProtocol::INIT_SPACE,
        seeds = [PROTOCOL_SEED],
        bump,
    )]
    pub protocol: Account<'info, ArgusProtocol>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_protocol(
    ctx: Context<InitializeProtocol>,
    license_fee_lamports: u64,
) -> Result<()> {
    let protocol = &mut ctx.accounts.protocol;
    protocol.version = STATE_VERSION;
    protocol.authority = ctx.accounts.authority.key();
    protocol.pending_authority = None;
    protocol.license_fee_lamports = license_fee_lamports;
    protocol.bump = ctx.bumps.protocol;
    emit!(ProtocolInitialized {
        authority: ctx.accounts.authority.key(),
        license_fee_lamports,
    });
    Ok(())
}

// ── protocol authority admin ─────────────────────────────────────────────────

#[derive(Accounts)]
pub struct ProtocolAuthorityOnly<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority @ GuardError::Unauthorized,
        seeds = [PROTOCOL_SEED],
        bump = protocol.bump,
    )]
    pub protocol: Account<'info, ArgusProtocol>,
}

pub fn handle_set_license_fee(ctx: Context<ProtocolAuthorityOnly>, fee: u64) -> Result<()> {
    ctx.accounts.protocol.license_fee_lamports = fee;
    Ok(())
}

pub fn handle_transfer_protocol_authority(
    ctx: Context<ProtocolAuthorityOnly>,
    new_authority: Pubkey,
) -> Result<()> {
    ctx.accounts.protocol.pending_authority = Some(new_authority);
    Ok(())
}

#[derive(Accounts)]
pub struct AcceptProtocolAuthority<'info> {
    pub pending_authority: Signer<'info>,

    #[account(mut, seeds = [PROTOCOL_SEED], bump = protocol.bump)]
    pub protocol: Account<'info, ArgusProtocol>,
}

pub fn handle_accept_protocol_authority(ctx: Context<AcceptProtocolAuthority>) -> Result<()> {
    let protocol = &mut ctx.accounts.protocol;
    require!(
        protocol.pending_authority == Some(ctx.accounts.pending_authority.key()),
        GuardError::PendingAuthorityMismatch
    );
    protocol.authority = ctx.accounts.pending_authority.key();
    protocol.pending_authority = None;
    Ok(())
}

/// Withdraw accrued license fees from the treasury PDA (protocol authority).
/// Manual lamport debit (the PDA is program-owned) keeping the account rent-
/// exempt so it can never be closed out from under the protocol.
#[derive(Accounts)]
pub struct WithdrawFees<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority @ GuardError::Unauthorized,
        seeds = [PROTOCOL_SEED],
        bump = protocol.bump,
    )]
    pub protocol: Account<'info, ArgusProtocol>,

    /// CHECK: fee recipient — any account.
    #[account(mut)]
    pub recipient: UncheckedAccount<'info>,
}

pub fn handle_withdraw_fees(ctx: Context<WithdrawFees>, amount: u64) -> Result<()> {
    let protocol_info = ctx.accounts.protocol.to_account_info();
    let rent_floor = Rent::get()?.minimum_balance(protocol_info.data_len());
    let available = protocol_info.lamports().saturating_sub(rent_floor);
    require!(amount <= available, GuardError::InsufficientFee);

    {
        let mut from = protocol_info.try_borrow_mut_lamports()?;
        **from = from
            .checked_sub(amount)
            .ok_or(GuardError::InsufficientFee)?;
    }
    {
        let mut to = ctx.accounts.recipient.try_borrow_mut_lamports()?;
        **to = to.checked_add(amount).ok_or(GuardError::Overflow)?;
    }

    emit!(FeesWithdrawn {
        recipient: ctx.accounts.recipient.key(),
        amount,
    });
    Ok(())
}

// ── set_license (protocol grants terms) ──────────────────────────────────────

#[derive(Accounts)]
pub struct SetLicense<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        has_one = authority @ GuardError::Unauthorized,
        seeds = [PROTOCOL_SEED],
        bump = protocol.bump,
    )]
    pub protocol: Account<'info, ArgusProtocol>,

    /// CHECK: the licensed mint — PDA seed only.
    pub mint: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = authority,
        space = 8 + LicenseState::INIT_SPACE,
        seeds = [LICENSE_SEED, mint.key().as_ref()],
        bump,
    )]
    pub license: Account<'info, LicenseState>,

    pub system_program: Program<'info, System>,
}

pub fn handle_set_license(
    ctx: Context<SetLicense>,
    tier: u8,
    entitlements: u32,
    expires_at: i64,
) -> Result<()> {
    require!(
        entitlements & !entitlement::ALL == 0,
        GuardError::InvalidEntitlement
    );
    require!(expires_at >= 0, GuardError::InvalidTimelock);
    let mint = ctx.accounts.mint.key();
    let license = &mut ctx.accounts.license;
    license.version = STATE_VERSION;
    license.mint = mint;
    license.tier = tier;
    license.entitlements = entitlements;
    license.expires_at = expires_at;
    license.bump = ctx.bumps.license;
    emit!(LicenseSet {
        mint,
        tier,
        entitlements,
        expires_at,
    });
    Ok(())
}

// ── purchase_license (tenant pays fee → treasury, extends expiry) ────────────

#[derive(Accounts)]
pub struct PurchaseLicense<'info> {
    /// The tenant's guard authority pays and signs.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: the licensed mint — PDA seed only.
    pub mint: UncheckedAccount<'info>,

    #[account(
        has_one = authority @ GuardError::Unauthorized,
        seeds = [GUARD_SEED, mint.key().as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,

    #[account(mut, seeds = [PROTOCOL_SEED], bump = protocol.bump)]
    pub protocol: Account<'info, ArgusProtocol>,

    #[account(
        mut,
        seeds = [LICENSE_SEED, mint.key().as_ref()],
        bump = license.bump,
    )]
    pub license: Account<'info, LicenseState>,

    pub system_program: Program<'info, System>,
}

pub fn handle_purchase_license(ctx: Context<PurchaseLicense>, periods: u32) -> Result<()> {
    require!(periods > 0, GuardError::InvalidEntitlement);
    let fee = ctx
        .accounts
        .protocol
        .license_fee_lamports
        .checked_mul(u64::from(periods))
        .ok_or(GuardError::Overflow)?;

    // Fee on a licensing event — paid by the tenant, routed to the treasury PDA.
    // Never a per-transfer charge, so holders are never taxed.
    if fee > 0 {
        transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.authority.to_account_info(),
                    to: ctx.accounts.protocol.to_account_info(),
                },
            ),
            fee,
        )?;
    }

    let now = Clock::get()?.unix_timestamp;
    let added = LICENSE_PERIOD_SECS
        .checked_mul(i64::from(periods))
        .ok_or(GuardError::Overflow)?;
    // Extend from now (lapsed) or from the current expiry (still live) — renewals
    // never lose remaining time.
    let license = &mut ctx.accounts.license;
    let base = license.expires_at.max(now);
    license.expires_at = base.checked_add(added).ok_or(GuardError::Overflow)?;

    emit!(LicensePurchased {
        mint: ctx.accounts.mint.key(),
        payer: ctx.accounts.authority.key(),
        fee_paid: fee,
        expires_at: license.expires_at,
    });
    Ok(())
}
