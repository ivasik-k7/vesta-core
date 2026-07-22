//! Verified customer segmentation (spec 12 §4.1, phase 1) — the growth moat.
//!
//! Brings aegis into the merchant using argus's verify-once/read-cheap pattern:
//! `refresh_customer_eligibility` pays aegis's `verify` CPI off the hot path and
//! caches the verdict as a `CustomerEligibility` bitmap; earn / offers / campaigns
//! then read a bit with no CPI. The merchant learns only that a predicate holds
//! (verified region / KYC / age / accredited), never the customer's PII — aegis
//! keeps only commitments on-chain. Additive and opt-in: a merchant that defines
//! no segments is unaffected, and a missing/stale cache simply reads as unmet.

use anchor_lang::{prelude::*, solana_program::program::get_return_data};

use crate::{
    constants::{
        CUSTOMER_ELIGIBILITY_SEED, DEFAULT_ELIGIBILITY_TTL_SECS, MAX_SEGMENTS, MERCHANT_SEED,
        SEGMENTS_SEED, STATE_VERSION,
    },
    error::VestaError,
    events::{CustomerEligibilityRefreshed, MerchantSegmentsSet},
    state::{CustomerEligibility, Merchant, MerchantSegments, Segment},
};

// ── set_merchant_segments (owner) ────────────────────────────────────────────

#[derive(Accounts)]
pub struct SetMerchantSegments<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        init_if_needed,
        payer = authority,
        space = 8 + MerchantSegments::INIT_SPACE,
        seeds = [SEGMENTS_SEED, merchant.key().as_ref()],
        bump,
    )]
    pub merchant_segments: Account<'info, MerchantSegments>,

    pub system_program: Program<'info, System>,
}

pub fn handle_set_merchant_segments(
    ctx: Context<SetMerchantSegments>,
    segments: Vec<Segment>,
) -> Result<()> {
    require!(segments.len() <= MAX_SEGMENTS, VestaError::TooManySegments);
    let merchant = ctx.accounts.merchant.key();

    let s = &mut ctx.accounts.merchant_segments;
    let first_init = s.merchant == Pubkey::default();
    if first_init {
        s.version = STATE_VERSION;
        s.merchant = merchant;
        s.policy_epoch = 0;
        s.bump = ctx.bumps.merchant_segments;
    }
    // Overwrite the whole set; unspecified slots reset to inactive default.
    s.segments = [Segment::default(); MAX_SEGMENTS];
    for (i, seg) in segments.iter().enumerate() {
        require!(seg.ttl_secs >= 0, VestaError::InvalidReserveParams);
        s.segments[i] = *seg;
    }
    // Any change invalidates every cached eligibility (epoch mismatch).
    s.policy_epoch = s.policy_epoch.saturating_add(1);

    emit!(MerchantSegmentsSet {
        merchant,
        policy_epoch: s.policy_epoch,
    });
    Ok(())
}

// ── refresh_customer_eligibility (permissionless crank) ──────────────────────

#[derive(Accounts)]
pub struct RefreshCustomerEligibility<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, merchant.authority.as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
    )]
    pub merchant: Account<'info, Merchant>,

    /// CHECK: the customer this cache certifies — PDA seed only.
    pub customer: UncheckedAccount<'info>,

    #[account(
        seeds = [SEGMENTS_SEED, merchant.key().as_ref()],
        bump = merchant_segments.bump,
    )]
    pub merchant_segments: Account<'info, MerchantSegments>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + CustomerEligibility::INIT_SPACE,
        seeds = [CUSTOMER_ELIGIBILITY_SEED, merchant.key().as_ref(), customer.key().as_ref()],
        bump,
    )]
    pub customer_eligibility: Account<'info, CustomerEligibility>,

    /// CHECK: the aegis attestation PDA for (segment issuer, customer, schema);
    /// aegis re-derives and owner-checks it — a wrong/missing account yields a
    /// negative verdict (fail safe).
    pub attestation: UncheckedAccount<'info>,

    /// CHECK: the aegis program — must be the canonical deployment.
    pub aegis_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_refresh_customer_eligibility(
    ctx: Context<RefreshCustomerEligibility>,
    segment_index: u8,
) -> Result<()> {
    let idx = usize::from(segment_index);
    require!(idx < MAX_SEGMENTS, VestaError::InvalidSegment);
    require_keys_eq!(
        ctx.accounts.aegis_program.key(),
        aegis::ID,
        VestaError::AegisProgramMismatch
    );

    let seg = ctx.accounts.merchant_segments.segments[idx];
    require!(seg.active, VestaError::InvalidSegment);

    let subject = ctx.accounts.customer.key();
    let predicate = aegis::VerifyPredicate::Present {
        issuer: seg.issuer,
        subject,
        schema_id: seg.schema_id,
    };
    let cpi = CpiContext::new(
        ctx.accounts.aegis_program.key(),
        aegis::cpi::accounts::Verify {
            attestation: ctx.accounts.attestation.to_account_info(),
        },
    );
    aegis::cpi::verify(cpi, predicate)?;

    let (returner, ret) = get_return_data().ok_or(VestaError::ConversionFailed)?;
    require_keys_eq!(returner, aegis::ID, VestaError::AegisProgramMismatch);
    let mut slice = ret.as_slice();
    let verdict =
        aegis::Verdict::deserialize(&mut slice).map_err(|_| VestaError::ConversionFailed)?;

    let now = Clock::get()?.unix_timestamp;
    let ttl = if seg.ttl_secs > 0 {
        seg.ttl_secs
    } else {
        DEFAULT_ELIGIBILITY_TTL_SECS
    };
    let expires_at = now.checked_add(ttl).ok_or(VestaError::Overflow)?;
    let epoch = ctx.accounts.merchant_segments.policy_epoch;
    let merchant = ctx.accounts.merchant.key();

    let cache = &mut ctx.accounts.customer_eligibility;
    // A stale-epoch cache (segments changed since) starts fresh.
    if cache.merchant == Pubkey::default() || cache.policy_epoch != epoch {
        cache.verdicts = 0;
    }
    cache.version = STATE_VERSION;
    cache.merchant = merchant;
    cache.customer = subject;
    let bit = 1u32 << segment_index;
    if verdict.ok {
        cache.verdicts |= bit;
    } else {
        cache.verdicts &= !bit;
    }
    cache.kyc_tier = verdict.tier;
    cache.jurisdiction = verdict.jurisdiction;
    cache.issued_at = now;
    cache.expires_at = expires_at;
    cache.policy_epoch = epoch;
    cache.aegis_program = aegis::ID;
    cache.bump = ctx.bumps.customer_eligibility;

    emit!(CustomerEligibilityRefreshed {
        merchant,
        customer: subject,
        segment_index,
        satisfied: verdict.ok,
        verdicts: cache.verdicts,
        expires_at,
    });
    Ok(())
}
