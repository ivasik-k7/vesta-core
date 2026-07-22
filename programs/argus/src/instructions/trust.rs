//! The trust triangle (spec 10, phase 3).
//!
//! argus's authority to gate a mint is *derived*, not asserted: the mint's
//! governing issuer must chain up to a configured aegis accreditation root. The
//! permissionless `reverify_accreditation` crank re-runs that check via aegis
//! `verify_accreditation` and, once a failing streak outlives the grace window,
//! trips `GuardConfig.degrade_mode` — so a revoked issuer's transfer authority
//! evaporates with no human key. Recovery auto-restores on the next healthy
//! crank. Degradation only blocks peer gifts; redemption and clawback stay open,
//! so a false-negative or aegis outage can never strand holder assets.

use anchor_lang::{prelude::*, solana_program::program::get_return_data};

use crate::{
    constants::{degrade, GUARD_SEED, STATE_VERSION, TRUST_SEED},
    error::GuardError,
    events::{AccreditationReverified, DegradeModeSet, ScreeningEpochBumped, TrustAnchorSet},
    state::{GuardConfig, TrustAnchor},
};

// ── set_trust_anchor ─────────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct SetTrustAnchor<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: the hooked mint — PDA seed only.
    pub mint: UncheckedAccount<'info>,

    #[account(
        has_one = authority @ GuardError::Unauthorized,
        seeds = [GUARD_SEED, mint.key().as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,

    #[account(
        init_if_needed,
        payer = authority,
        space = 8 + TrustAnchor::INIT_SPACE,
        seeds = [TRUST_SEED, mint.key().as_ref()],
        bump,
    )]
    pub trust_anchor: Account<'info, TrustAnchor>,

    pub system_program: Program<'info, System>,
}

pub fn handle_set_trust_anchor(
    ctx: Context<SetTrustAnchor>,
    accreditation_root: Pubkey,
    subject_issuer: Pubkey,
    required_schema: u64,
    degrade_target: u8,
    grace_secs: i64,
) -> Result<()> {
    require!(
        degrade::is_valid_target(degrade_target),
        GuardError::InvalidDegradeTarget
    );
    require!(grace_secs >= 0, GuardError::InvalidTimelock);
    require_keys_neq!(
        accreditation_root,
        Pubkey::default(),
        GuardError::TrustAnchorMissing
    );

    let now = Clock::get()?.unix_timestamp;
    let mint = ctx.accounts.mint.key();
    let aegis_program = ctx.accounts.guard_config.aegis_program;

    let anchor = &mut ctx.accounts.trust_anchor;
    anchor.version = STATE_VERSION;
    anchor.mint = mint;
    anchor.accreditation_root = accreditation_root;
    anchor.subject_issuer = subject_issuer;
    anchor.required_schema = required_schema;
    anchor.aegis_program = aegis_program;
    anchor.degrade_target = degrade_target;
    anchor.grace_secs = grace_secs;
    // Setting/re-setting the anchor resets the health state to a clean, healthy
    // baseline; the next crank re-establishes the verdict.
    anchor.failing_since = 0;
    anchor.last_verified_at = now;
    anchor.bump = ctx.bumps.trust_anchor;

    emit!(TrustAnchorSet {
        mint,
        accreditation_root,
        subject_issuer,
        required_schema,
        degrade_target,
    });
    Ok(())
}

// ── reverify_accreditation (permissionless crank) ────────────────────────────

#[derive(Accounts)]
pub struct ReverifyAccreditation<'info> {
    /// Anyone may crank — the check is deterministic and mutates only health
    /// state; there is no privileged input to abuse.
    pub cranker: Signer<'info>,

    /// CHECK: the hooked mint — PDA seed only.
    pub mint: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [GUARD_SEED, mint.key().as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,

    #[account(
        mut,
        seeds = [TRUST_SEED, mint.key().as_ref()],
        bump = trust_anchor.bump,
    )]
    pub trust_anchor: Account<'info, TrustAnchor>,

    /// CHECK: aegis TrustRoot PDA — passed through to aegis, which re-derives
    /// and owner-checks it; a wrong account yields a negative verdict (fail safe).
    pub aegis_trust_root: UncheckedAccount<'info>,

    /// CHECK: aegis Accreditation PDA — likewise re-derived + owner-checked by aegis.
    pub aegis_accreditation: UncheckedAccount<'info>,

    /// CHECK: the aegis program — must equal the guard's configured deployment.
    pub aegis_program: UncheckedAccount<'info>,
}

pub fn handle_reverify_accreditation(ctx: Context<ReverifyAccreditation>) -> Result<()> {
    let anchor = &ctx.accounts.trust_anchor;
    require_keys_eq!(
        ctx.accounts.aegis_program.key(),
        anchor.aegis_program,
        GuardError::AegisProgramMismatch
    );

    // Pin the aegis accounts to their canonical PDAs (shared-conventions
    // invariant #3). The crank is permissionless, so if we merely passed
    // caller-supplied accounts through, a griefer could force a NOT_ACCREDITED
    // verdict — and, after grace, an auto-degrade — by cranking with the wrong
    // accounts. Deriving them from the anchor's own root/subject makes the crank
    // deterministic: honest or not, it can only reflect the real on-chain state.
    let expected_root = Pubkey::find_program_address(
        &[b"troot", anchor.accreditation_root.as_ref()],
        &anchor.aegis_program,
    )
    .0;
    require_keys_eq!(
        ctx.accounts.aegis_trust_root.key(),
        expected_root,
        GuardError::MetaListMismatch
    );
    let expected_accreditation = Pubkey::find_program_address(
        &[
            b"accred",
            anchor.accreditation_root.as_ref(),
            anchor.subject_issuer.as_ref(),
        ],
        &anchor.aegis_program,
    )
    .0;
    require_keys_eq!(
        ctx.accounts.aegis_accreditation.key(),
        expected_accreditation,
        GuardError::MetaListMismatch
    );

    // Ask aegis whether the governing issuer is still accredited. Never reverts
    // on a negative — aegis returns the verdict via return-data.
    let cpi = CpiContext::new(
        ctx.accounts.aegis_program.key(),
        aegis::cpi::accounts::VerifyAccreditation {
            trust_root: ctx.accounts.aegis_trust_root.to_account_info(),
            accreditation: ctx.accounts.aegis_accreditation.to_account_info(),
        },
    );
    aegis::cpi::verify_accreditation(
        cpi,
        anchor.accreditation_root,
        anchor.subject_issuer,
        anchor.required_schema,
    )?;

    let (returner, ret) = get_return_data().ok_or(GuardError::EligibilityStale)?;
    require_keys_eq!(
        returner,
        anchor.aegis_program,
        GuardError::AegisProgramMismatch
    );
    let mut slice = ret.as_slice();
    let verdict =
        aegis::Verdict::deserialize(&mut slice).map_err(|_| GuardError::EligibilityStale)?;

    let now = Clock::get()?.unix_timestamp;
    let grace = anchor.grace_secs;
    let degrade_target = anchor.degrade_target;
    let old_mode = ctx.accounts.guard_config.degrade_mode;
    let mint = anchor.mint;

    let anchor = &mut ctx.accounts.trust_anchor;
    let new_mode = if verdict.ok {
        // Healthy — clear any failing streak and auto-restore the posture.
        anchor.failing_since = 0;
        anchor.last_verified_at = now;
        degrade::NORMAL
    } else {
        // Failing — start (or continue) the streak; degrade only once the grace
        // window is exhausted, so a transient aegis outage doesn't brick a mint.
        if anchor.failing_since == 0 {
            anchor.failing_since = now;
        }
        let elapsed = now.checked_sub(anchor.failing_since).unwrap_or(0);
        if elapsed >= grace {
            degrade_target
        } else {
            old_mode
        }
    };

    ctx.accounts.guard_config.degrade_mode = new_mode;

    emit!(AccreditationReverified {
        mint,
        healthy: verdict.ok,
        degrade_mode: new_mode,
        reason_code: verdict.reason_code,
    });
    if new_mode != old_mode {
        emit!(DegradeModeSet {
            mint,
            old: old_mode,
            new: new_mode,
            automatic: true,
        });
    }
    Ok(())
}

// ── set_degrade_mode (manual override / challenge path) ──────────────────────

#[derive(Accounts)]
pub struct SetDegradeMode<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: the hooked mint — PDA seed only.
    pub mint: UncheckedAccount<'info>,

    #[account(
        mut,
        has_one = authority @ GuardError::Unauthorized,
        seeds = [GUARD_SEED, mint.key().as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,

    #[account(
        mut,
        seeds = [TRUST_SEED, mint.key().as_ref()],
        bump = trust_anchor.bump,
    )]
    pub trust_anchor: Account<'info, TrustAnchor>,
}

/// Guard authority manually sets the posture — an emergency degrade, or a
/// `NORMAL` restore after resolving a dispute (spec 10 §4.3 degrade/restore, §7
/// challenge path). A manual restore also clears the failing streak so the next
/// crank starts clean.
pub fn handle_set_degrade_mode(ctx: Context<SetDegradeMode>, mode: u8) -> Result<()> {
    require!(
        mode == degrade::NORMAL || degrade::is_valid_target(mode),
        GuardError::InvalidDegradeTarget
    );
    let mint = ctx.accounts.mint.key();
    let old = ctx.accounts.guard_config.degrade_mode;
    ctx.accounts.guard_config.degrade_mode = mode;
    if mode == degrade::NORMAL {
        ctx.accounts.trust_anchor.failing_since = 0;
    }
    emit!(DegradeModeSet {
        mint,
        old,
        new: mode,
        automatic: false,
    });
    Ok(())
}

// ── bump_screening_epoch (SANCTIONS fast-freeze) ─────────────────────────────

#[derive(Accounts)]
pub struct BumpScreeningEpoch<'info> {
    pub authority: Signer<'info>,

    /// CHECK: the hooked mint — PDA seed only.
    pub mint: UncheckedAccount<'info>,

    #[account(
        mut,
        has_one = authority @ GuardError::Unauthorized,
        seeds = [GUARD_SEED, mint.key().as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,
}

/// Advance the screening epoch (spec 10 §4.4). Every cached `EligibilityCapability`
/// of this mint instantly becomes stale — the next transfer that requires
/// eligibility fails closed until a fresh `refresh_eligibility` re-runs aegis's
/// (sanctions-aware) `verify`/`verify_policy`. This is the near-real-time
/// freeze lever: it does not wait for TTL and does not touch policy_epoch.
pub fn handle_bump_screening_epoch(ctx: Context<BumpScreeningEpoch>) -> Result<()> {
    let config = &mut ctx.accounts.guard_config;
    config.screening_epoch = config.screening_epoch.saturating_add(1);
    emit!(ScreeningEpochBumped {
        mint: config.mint,
        screening_epoch: config.screening_epoch,
    });
    Ok(())
}
