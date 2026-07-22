use anchor_lang::{prelude::*, solana_program::program::get_return_data};

use crate::{
    constants::{
        CAPABILITY_TTL_SECS, CAP_SEED, GUARD_SEED, PREDICATE_ATTESTATION_BIT, STATE_VERSION,
    },
    error::GuardError,
    events::{CapabilityInvalidated, EligibilityRefreshed},
    state::{EligibilityCapability, GuardConfig},
};

/// Off-hot-path: pay aegis's `verify` CPI once and cache the verdict as an
/// `EligibilityCapability` (spec 09 §4.1). `execute` then reads the cached
/// bitmap with no CPI. Permissionless — anyone (usually the sender/relayer,
/// often bundled with the transfer) may refresh a subject's capability.
#[derive(Accounts)]
pub struct RefreshEligibility<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: the hooked mint — used only as a PDA seed.
    pub mint: UncheckedAccount<'info>,

    #[account(
        seeds = [GUARD_SEED, mint.key().as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,

    /// CHECK: the subject wallet this capability certifies.
    pub subject: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + EligibilityCapability::INIT_SPACE,
        seeds = [CAP_SEED, mint.key().as_ref(), subject.key().as_ref()],
        bump,
    )]
    pub capability: Account<'info, EligibilityCapability>,

    /// CHECK: the aegis attestation PDA for (issuer, subject, schema). Passed
    /// through to aegis `verify`/`verify_policy`, which re-derive and owner-check
    /// it; a wrong or missing account yields a negative verdict (fail safe).
    pub attestation: UncheckedAccount<'info>,

    /// CHECK: the aegis `Policy` account to enforce, when the guard configures
    /// one (`guard_config.policy`). Ignored on the legacy `Present` path.
    pub policy: UncheckedAccount<'info>,

    /// CHECK: the aegis program — must equal the guard's configured deployment.
    pub aegis_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_refresh_eligibility(ctx: Context<RefreshEligibility>) -> Result<()> {
    let gc = &ctx.accounts.guard_config;
    require_keys_eq!(
        ctx.accounts.aegis_program.key(),
        gc.aegis_program,
        GuardError::AegisProgramMismatch
    );

    // Ask aegis for a verdict on the subject. If the guard enforces an aegis
    // `Policy`, delegate the whole decision (jurisdiction / schema / freshness)
    // to `verify_policy` — so the compliance rule is editable in aegis as data,
    // no argus redeploy. Otherwise use the legacy single-credential `Present`
    // check. Either way the verdict returns via return-data and never reverts on
    // a negative result.
    let subject_key = ctx.accounts.subject.key();
    if gc.policy != Pubkey::default() {
        require_keys_eq!(
            ctx.accounts.policy.key(),
            gc.policy,
            GuardError::MetaListMismatch
        );
        let cpi = CpiContext::new(
            ctx.accounts.aegis_program.key(),
            aegis::cpi::accounts::VerifyPolicy {
                policy: ctx.accounts.policy.to_account_info(),
                attestation: ctx.accounts.attestation.to_account_info(),
            },
        );
        aegis::cpi::verify_policy(cpi, subject_key)?;
    } else {
        let predicate = aegis::VerifyPredicate::Present {
            issuer: gc.attestation_issuer,
            subject: subject_key,
            schema_id: gc.attestation_schema,
        };
        let cpi = CpiContext::new(
            ctx.accounts.aegis_program.key(),
            aegis::cpi::accounts::Verify {
                attestation: ctx.accounts.attestation.to_account_info(),
            },
        );
        aegis::cpi::verify(cpi, predicate)?;
    }

    let (returner, ret) = get_return_data().ok_or(GuardError::EligibilityStale)?;
    require_keys_eq!(returner, gc.aegis_program, GuardError::AegisProgramMismatch);
    let mut slice = ret.as_slice();
    let verdict =
        aegis::Verdict::deserialize(&mut slice).map_err(|_| GuardError::EligibilityStale)?;

    let verdicts = if verdict.ok {
        PREDICATE_ATTESTATION_BIT
    } else {
        0
    };
    let ttl = if gc.capability_ttl_secs > 0 {
        gc.capability_ttl_secs
    } else {
        CAPABILITY_TTL_SECS
    };
    let now = Clock::get()?.unix_timestamp;
    let expires_at = now.checked_add(ttl).ok_or(GuardError::Overflow)?;

    let mint = ctx.accounts.mint.key();
    let subject = ctx.accounts.subject.key();
    let aegis_program = gc.aegis_program;
    let policy_epoch = gc.policy_epoch;
    let screening_epoch = gc.screening_epoch;
    let bump = ctx.bumps.capability;
    let jurisdiction = verdict.jurisdiction;
    let tier = verdict.tier;

    let cap = &mut ctx.accounts.capability;
    cap.version = STATE_VERSION;
    cap.mint = mint;
    cap.subject = subject;
    cap.verdicts = verdicts;
    cap.aegis_program = aegis_program;
    cap.policy_epoch = policy_epoch;
    cap.screening_epoch = screening_epoch;
    cap.jurisdiction = jurisdiction;
    cap.tier = tier;
    cap.issued_at = now;
    cap.expires_at = expires_at;
    cap.bump = bump;

    emit!(EligibilityRefreshed {
        mint,
        subject,
        verdicts,
        expires_at,
    });
    Ok(())
}

/// Guard-authority: immediately invalidate a subject's cached capability
/// (spec 09). Closes the aegis-revocation-latency window for a *known* revoked
/// subject without the global `configure_policy` epoch bump that would nuke
/// every subject's cache.
#[derive(Accounts)]
pub struct InvalidateCapability<'info> {
    pub authority: Signer<'info>,

    /// CHECK: the hooked mint — PDA seed only.
    pub mint: UncheckedAccount<'info>,

    #[account(
        has_one = authority @ GuardError::Unauthorized,
        seeds = [GUARD_SEED, mint.key().as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,

    /// CHECK: subject whose capability is being invalidated — PDA seed only.
    pub subject: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [CAP_SEED, mint.key().as_ref(), subject.key().as_ref()],
        bump = capability.bump,
    )]
    pub capability: Account<'info, EligibilityCapability>,
}

pub fn handle_invalidate_capability(ctx: Context<InvalidateCapability>) -> Result<()> {
    let cap = &mut ctx.accounts.capability;
    cap.verdicts = 0;
    cap.expires_at = 0;
    emit!(CapabilityInvalidated {
        mint: ctx.accounts.mint.key(),
        subject: ctx.accounts.subject.key(),
    });
    Ok(())
}
