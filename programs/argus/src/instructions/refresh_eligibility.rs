use anchor_lang::{prelude::*, solana_program::program::get_return_data};

use crate::{
    constants::{
        CAPABILITY_TTL_SECS, CAP_SEED, GUARD_SEED, PREDICATE_ATTESTATION_BIT, STATE_VERSION,
    },
    error::GuardError,
    events::EligibilityRefreshed,
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
    /// through to aegis `verify`, which re-derives and owner-checks it; a wrong
    /// or missing account yields a negative verdict (fail safe), not an error.
    pub attestation: UncheckedAccount<'info>,

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

    // Ask aegis whether the subject holds a valid credential of the configured
    // schema from the trusted issuer (spec 07 `Present`). The verdict returns
    // via return-data; `verify` never reverts on a negative result.
    let predicate = aegis::VerifyPredicate::Present {
        issuer: gc.attestation_issuer,
        subject: ctx.accounts.subject.key(),
        schema_id: gc.attestation_schema,
    };
    let cpi = CpiContext::new(
        ctx.accounts.aegis_program.key(),
        aegis::cpi::accounts::Verify {
            attestation: ctx.accounts.attestation.to_account_info(),
        },
    );
    aegis::cpi::verify(cpi, predicate)?;

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
    let now = Clock::get()?.unix_timestamp;
    let expires_at = now
        .checked_add(CAPABILITY_TTL_SECS)
        .ok_or(GuardError::Overflow)?;

    let mint = ctx.accounts.mint.key();
    let subject = ctx.accounts.subject.key();
    let aegis_program = gc.aegis_program;
    let policy_epoch = gc.policy_epoch;
    let bump = ctx.bumps.capability;

    let cap = &mut ctx.accounts.capability;
    cap.version = STATE_VERSION;
    cap.mint = mint;
    cap.subject = subject;
    cap.verdicts = verdicts;
    cap.aegis_program = aegis_program;
    cap.policy_epoch = policy_epoch;
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
