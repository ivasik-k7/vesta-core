//! Accredited merchant identity + auto-degrade (spec 11 §4.1, phase 1).
//!
//! The merchant's authority to *issue* points is derived from an aegis
//! accreditation root, not asserted by an admin bool. A permissionless crank
//! re-checks the chain via aegis `verify_accreditation` and, once a failing
//! streak outlives the grace window, freezes issuance (`Merchant.issue_status`)
//! — a near-verbatim port of the shipped argus trust triangle. Degradation
//! freezes earn only; redemption and clawback stay open, so a revoked or
//! transiently-unreachable accreditation never strands holder assets. Opt-in and
//! additive: a merchant that never sets a trust anchor stays `NORMAL` forever.

use anchor_lang::{prelude::*, solana_program::program::get_return_data};

use crate::{
    constants::{issue_status, MERCHANT_SEED, MERCHANT_TRUST_SEED},
    error::VestaError,
    events::{MerchantIssueStatusSet, MerchantReverified, MerchantTrustSet},
    state::{Merchant, MerchantTrust},
};

/// aegis PDA seeds (aegis does not export them as a linkable const here; they are
/// part of its stable account ABI, cross-checked by the reverify integration test).
const AEGIS_TRUST_ROOT_SEED: &[u8] = b"troot";
const AEGIS_ACCREDITATION_SEED: &[u8] = b"accred";

// ── set_merchant_trust (owner) ───────────────────────────────────────────────

#[derive(Accounts)]
pub struct SetMerchantTrust<'info> {
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
        space = 8 + MerchantTrust::INIT_SPACE,
        seeds = [MERCHANT_TRUST_SEED, merchant.key().as_ref()],
        bump,
    )]
    pub merchant_trust: Account<'info, MerchantTrust>,

    pub system_program: Program<'info, System>,
}

pub fn handle_set_merchant_trust(
    ctx: Context<SetMerchantTrust>,
    accreditation_root: Pubkey,
    subject_issuer: Pubkey,
    required_schema: u64,
    aegis_program: Pubkey,
    degrade_target: u8,
    grace_secs: i64,
) -> Result<()> {
    require!(
        issue_status::is_valid_target(degrade_target),
        VestaError::InvalidIssueStatus
    );
    require!(grace_secs >= 0, VestaError::InvalidGrace);
    require_keys_neq!(
        accreditation_root,
        Pubkey::default(),
        VestaError::MerchantTrustMissing
    );
    require_keys_neq!(
        aegis_program,
        Pubkey::default(),
        VestaError::AegisProgramMismatch
    );

    let now = Clock::get()?.unix_timestamp;
    let merchant = ctx.accounts.merchant.key();

    let t = &mut ctx.accounts.merchant_trust;
    t.version = crate::constants::STATE_VERSION;
    t.merchant = merchant;
    t.accreditation_root = accreditation_root;
    t.subject_issuer = subject_issuer;
    t.required_schema = required_schema;
    t.aegis_program = aegis_program;
    t.degrade_target = degrade_target;
    t.grace_secs = grace_secs;
    t.failing_since = 0;
    t.last_verified_at = now;
    t.bump = ctx.bumps.merchant_trust;

    emit!(MerchantTrustSet {
        merchant,
        accreditation_root,
        subject_issuer,
        required_schema,
        degrade_target,
    });
    Ok(())
}

// ── reverify_merchant (permissionless crank) ─────────────────────────────────

#[derive(Accounts)]
pub struct ReverifyMerchant<'info> {
    pub cranker: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant.authority.as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        mut,
        seeds = [MERCHANT_TRUST_SEED, merchant.key().as_ref()],
        bump = merchant_trust.bump,
    )]
    pub merchant_trust: Account<'info, MerchantTrust>,

    /// CHECK: aegis TrustRoot PDA — re-derived + owner-checked by aegis; pinned
    /// here from the anchor's own root so a crank cannot substitute accounts.
    pub aegis_trust_root: UncheckedAccount<'info>,

    /// CHECK: aegis Accreditation PDA — likewise re-derived + owner-checked by aegis.
    pub aegis_accreditation: UncheckedAccount<'info>,

    /// CHECK: the aegis program — must equal the merchant's configured deployment.
    pub aegis_program: UncheckedAccount<'info>,
}

pub fn handle_reverify_merchant(ctx: Context<ReverifyMerchant>) -> Result<()> {
    let t = &ctx.accounts.merchant_trust;
    require_keys_eq!(
        ctx.accounts.aegis_program.key(),
        t.aegis_program,
        VestaError::AegisProgramMismatch
    );

    // Pin the aegis accounts to their canonical PDAs (invariant #3): the crank is
    // permissionless, so deriving from the anchor's own root/subject makes the
    // verdict deterministic — a griefer cannot force a NOT_ACCREDITED result.
    let expected_root = Pubkey::find_program_address(
        &[AEGIS_TRUST_ROOT_SEED, t.accreditation_root.as_ref()],
        &t.aegis_program,
    )
    .0;
    require_keys_eq!(
        ctx.accounts.aegis_trust_root.key(),
        expected_root,
        VestaError::MerchantTrustMissing
    );
    let expected_acc = Pubkey::find_program_address(
        &[
            AEGIS_ACCREDITATION_SEED,
            t.accreditation_root.as_ref(),
            t.subject_issuer.as_ref(),
        ],
        &t.aegis_program,
    )
    .0;
    require_keys_eq!(
        ctx.accounts.aegis_accreditation.key(),
        expected_acc,
        VestaError::MerchantTrustMissing
    );

    let cpi = CpiContext::new(
        ctx.accounts.aegis_program.key(),
        aegis::cpi::accounts::VerifyAccreditation {
            trust_root: ctx.accounts.aegis_trust_root.to_account_info(),
            accreditation: ctx.accounts.aegis_accreditation.to_account_info(),
        },
    );
    aegis::cpi::verify_accreditation(
        cpi,
        t.accreditation_root,
        t.subject_issuer,
        t.required_schema,
    )?;

    let (returner, ret) = get_return_data().ok_or(VestaError::ConversionFailed)?;
    require_keys_eq!(returner, t.aegis_program, VestaError::AegisProgramMismatch);
    let mut slice = ret.as_slice();
    let verdict =
        aegis::Verdict::deserialize(&mut slice).map_err(|_| VestaError::ConversionFailed)?;

    let now = Clock::get()?.unix_timestamp;
    let grace = t.grace_secs;
    let degrade_target = t.degrade_target;
    let old_status = ctx.accounts.merchant.issue_status;
    let merchant_key = ctx.accounts.merchant.key();

    let (tier, jurisdiction) = (verdict.tier, verdict.jurisdiction);
    let t = &mut ctx.accounts.merchant_trust;
    let new_status = if verdict.ok {
        t.failing_since = 0;
        t.last_verified_at = now;
        t.tier = tier;
        t.jurisdiction = jurisdiction;
        issue_status::NORMAL
    } else {
        if t.failing_since == 0 {
            t.failing_since = now;
        }
        let elapsed = now.checked_sub(t.failing_since).unwrap_or(0);
        if elapsed >= grace {
            degrade_target
        } else {
            old_status
        }
    };

    ctx.accounts.merchant.issue_status = new_status;

    emit!(MerchantReverified {
        merchant: merchant_key,
        healthy: verdict.ok,
        issue_status: new_status,
        reason_code: verdict.reason_code,
    });
    if new_status != old_status {
        emit!(MerchantIssueStatusSet {
            merchant: merchant_key,
            old: old_status,
            new: new_status,
            automatic: true,
        });
    }
    Ok(())
}

// ── set_merchant_issue_status (owner manual override / dispute restore) ──────

#[derive(Accounts)]
pub struct SetMerchantIssueStatus<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        mut,
        seeds = [MERCHANT_TRUST_SEED, merchant.key().as_ref()],
        bump = merchant_trust.bump,
    )]
    pub merchant_trust: Account<'info, MerchantTrust>,
}

pub fn handle_set_merchant_issue_status(
    ctx: Context<SetMerchantIssueStatus>,
    status: u8,
) -> Result<()> {
    require!(
        status == issue_status::NORMAL || issue_status::is_valid_target(status),
        VestaError::InvalidIssueStatus
    );
    let merchant_key = ctx.accounts.merchant.key();
    let old = ctx.accounts.merchant.issue_status;
    ctx.accounts.merchant.issue_status = status;
    if status == issue_status::NORMAL {
        ctx.accounts.merchant_trust.failing_since = 0;
    }
    emit!(MerchantIssueStatusSet {
        merchant: merchant_key,
        old,
        new: status,
        automatic: false,
    });
    Ok(())
}
