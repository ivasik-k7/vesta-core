//! Point-liability reserve & proof-of-reserves (spec 11 §4.2, phase 2).
//!
//! Escrows a caller-chosen stablecoin against outstanding point liability. The
//! coverage invariant is enforced where value leaves — a withdrawal may never
//! drop the reserve below what is required to back the point mint's current raw
//! supply — and `attest_reserve` publishes a permissionless, examiner-facing
//! proof-of-reserves snapshot. Liability is measured on raw supply, which is
//! exact and decay-conservative (decay lowers UI value while raw supply is
//! unchanged, so backing raw over-collateralizes rather than under).
//!
//! Opt-in and additive: a merchant that never opens a reserve is unaffected. The
//! pre-mint solvency gate on `earn` is a documented follow-up (it would thread
//! optional reserve accounts through the earn hot path and every earn call site);
//! withdrawal-coverage + public attestation already guarantee that escrowed
//! reserves cannot be pulled below outstanding liability and that
//! under-collateralization is publicly provable.

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};

use crate::{
    constants::{MERCHANT_RESERVE_SEED, MERCHANT_SEED},
    error::VestaError,
    events::{ReserveAttested, ReserveFunded, ReserveOpened, ReserveWithdrawn},
    state::{Merchant, MerchantReserve},
};

// ── open_reserve (owner) ─────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct OpenReserve<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    pub backing_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = authority,
        space = 8 + MerchantReserve::INIT_SPACE,
        seeds = [MERCHANT_RESERVE_SEED, merchant.key().as_ref()],
        bump,
    )]
    pub merchant_reserve: Account<'info, MerchantReserve>,

    #[account(
        init,
        payer = authority,
        associated_token::mint = backing_mint,
        associated_token::authority = merchant_reserve,
        associated_token::token_program = token_program,
    )]
    pub reserve_ata: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_open_reserve(
    ctx: Context<OpenReserve>,
    unit_value: u64,
    target_ratio_bps: u16,
) -> Result<()> {
    require!(unit_value > 0, VestaError::InvalidReserveParams);
    require!(
        target_ratio_bps > 0 && u64::from(target_ratio_bps) <= 65_535,
        VestaError::InvalidReserveParams
    );
    let merchant = ctx.accounts.merchant.key();
    let r = &mut ctx.accounts.merchant_reserve;
    r.version = crate::constants::STATE_VERSION;
    r.merchant = merchant;
    r.backing_mint = ctx.accounts.backing_mint.key();
    r.reserve_ata = ctx.accounts.reserve_ata.key();
    r.unit_value = unit_value;
    r.target_ratio_bps = target_ratio_bps;
    r.bump = ctx.bumps.merchant_reserve;
    emit!(ReserveOpened {
        merchant,
        backing_mint: r.backing_mint,
        unit_value,
        target_ratio_bps,
    });
    Ok(())
}

// ── fund_reserve (owner) ─────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct FundReserve<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        seeds = [MERCHANT_RESERVE_SEED, merchant.key().as_ref()],
        bump = merchant_reserve.bump,
        has_one = backing_mint @ VestaError::ReserveMintMismatch,
        has_one = reserve_ata @ VestaError::ReserveMintMismatch,
    )]
    pub merchant_reserve: Account<'info, MerchantReserve>,

    pub backing_mint: InterfaceAccount<'info, Mint>,

    #[account(mut)]
    pub source_ata: InterfaceAccount<'info, TokenAccount>,

    #[account(mut)]
    pub reserve_ata: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_fund_reserve(ctx: Context<FundReserve>, amount: u64) -> Result<()> {
    require!(amount > 0, VestaError::InvalidAmount);
    transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.source_ata.to_account_info(),
                mint: ctx.accounts.backing_mint.to_account_info(),
                to: ctx.accounts.reserve_ata.to_account_info(),
                authority: ctx.accounts.authority.to_account_info(),
            },
        ),
        amount,
        ctx.accounts.backing_mint.decimals,
    )?;
    ctx.accounts.reserve_ata.reload()?;
    emit!(ReserveFunded {
        merchant: ctx.accounts.merchant.key(),
        amount,
        reserve_balance: ctx.accounts.reserve_ata.amount,
    });
    Ok(())
}

// ── withdraw_reserve (owner, coverage-enforced) ──────────────────────────────

#[derive(Accounts)]
pub struct WithdrawReserve<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Account<'info, Merchant>,

    /// The point mint — its raw `supply` is the outstanding liability.
    pub point_mint: InterfaceAccount<'info, Mint>,

    #[account(
        seeds = [MERCHANT_RESERVE_SEED, merchant.key().as_ref()],
        bump = merchant_reserve.bump,
        has_one = backing_mint @ VestaError::ReserveMintMismatch,
        has_one = reserve_ata @ VestaError::ReserveMintMismatch,
    )]
    pub merchant_reserve: Account<'info, MerchantReserve>,

    pub backing_mint: InterfaceAccount<'info, Mint>,

    #[account(mut)]
    pub reserve_ata: InterfaceAccount<'info, TokenAccount>,

    #[account(mut)]
    pub destination_ata: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_withdraw_reserve(ctx: Context<WithdrawReserve>, amount: u64) -> Result<()> {
    require!(amount > 0, VestaError::InvalidAmount);

    let merchant_key = ctx.accounts.merchant.key();
    let signer_seeds: &[&[u8]] = &[
        MERCHANT_RESERVE_SEED,
        merchant_key.as_ref(),
        &[ctx.accounts.merchant_reserve.bump],
    ];
    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.reserve_ata.to_account_info(),
                mint: ctx.accounts.backing_mint.to_account_info(),
                to: ctx.accounts.destination_ata.to_account_info(),
                authority: ctx.accounts.merchant_reserve.to_account_info(),
            },
            &[signer_seeds],
        ),
        amount,
        ctx.accounts.backing_mint.decimals,
    )?;
    ctx.accounts.reserve_ata.reload()?;

    // Coverage invariant: after the withdrawal, the reserve must still back the
    // point mint's current raw supply at the configured ratio.
    let required = ctx
        .accounts
        .merchant_reserve
        .required_reserve(ctx.accounts.point_mint.supply)
        .ok_or(VestaError::Overflow)?;
    require!(
        ctx.accounts.reserve_ata.amount >= required,
        VestaError::ReserveCoverageBreach
    );

    emit!(ReserveWithdrawn {
        merchant: merchant_key,
        amount,
        reserve_balance: ctx.accounts.reserve_ata.amount,
    });
    Ok(())
}

// ── attest_reserve (permissionless proof-of-reserves) ────────────────────────

#[derive(Accounts)]
pub struct AttestReserve<'info> {
    #[account(
        seeds = [MERCHANT_SEED, merchant.authority.as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Account<'info, Merchant>,

    pub point_mint: InterfaceAccount<'info, Mint>,

    #[account(
        seeds = [MERCHANT_RESERVE_SEED, merchant.key().as_ref()],
        bump = merchant_reserve.bump,
        has_one = reserve_ata @ VestaError::ReserveMintMismatch,
    )]
    pub merchant_reserve: Account<'info, MerchantReserve>,

    pub reserve_ata: InterfaceAccount<'info, TokenAccount>,
}

pub fn handle_attest_reserve(ctx: Context<AttestReserve>) -> Result<()> {
    let outstanding_raw = ctx.accounts.point_mint.supply;
    let reserve_stable = ctx.accounts.reserve_ata.amount;
    let required_stable = ctx
        .accounts
        .merchant_reserve
        .required_reserve(outstanding_raw)
        .ok_or(VestaError::Overflow)?;
    emit!(ReserveAttested {
        merchant: ctx.accounts.merchant.key(),
        outstanding_raw,
        reserve_stable,
        required_stable,
        solvent: reserve_stable >= required_stable,
        ts: Clock::get()?.unix_timestamp,
    });
    Ok(())
}
