use anchor_lang::prelude::*;

use crate::{
    constants::{POLICY_SEED, STATE_VERSION},
    error::AegisError,
    events::{PolicyDecision, PolicyDeprecated, PolicyRegistered},
    instructions::verify::{emit_verdict, evaluate, VerifyPredicate},
    state::Policy,
};

/// Register a named, versioned verifier policy: "a valid credential of
/// `schema_id` from `issuer`, no older than `freshness_secs`, in `jurisdiction`".
#[derive(Accounts)]
#[instruction(id: u64)]
pub struct RegisterPolicy<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + Policy::INIT_SPACE,
        seeds = [POLICY_SEED, authority.key().as_ref(), &id.to_le_bytes()],
        bump,
    )]
    pub policy: Account<'info, Policy>,

    pub system_program: Program<'info, System>,
}

pub fn handle_register_policy(
    ctx: Context<RegisterPolicy>,
    id: u64,
    jurisdiction: u16,
    issuer: Pubkey,
    schema_id: u64,
    freshness_secs: i64,
) -> Result<()> {
    require!(freshness_secs >= 0, AegisError::InvalidValidFrom);
    let policy = &mut ctx.accounts.policy;
    policy.version = STATE_VERSION;
    policy.authority = ctx.accounts.authority.key();
    policy.id = id;
    policy.jurisdiction = jurisdiction;
    policy.issuer = issuer;
    policy.schema_id = schema_id;
    policy.freshness_secs = freshness_secs;
    policy.deprecated = false;
    policy.successor = None;
    policy.bump = ctx.bumps.policy;

    emit!(PolicyRegistered {
        policy: policy.key(),
        authority: policy.authority,
        id,
        jurisdiction,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct DeprecatePolicy<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority @ AegisError::Unauthorized,
        seeds = [POLICY_SEED, authority.key().as_ref(), &policy.id.to_le_bytes()],
        bump = policy.bump,
    )]
    pub policy: Account<'info, Policy>,
}

pub fn handle_deprecate_policy(
    ctx: Context<DeprecatePolicy>,
    successor: Option<Pubkey>,
) -> Result<()> {
    let policy = &mut ctx.accounts.policy;
    policy.deprecated = true;
    policy.successor = successor;
    emit!(PolicyDeprecated {
        policy: policy.key(),
        successor,
    });
    Ok(())
}

/// Evaluate a subject against a named policy. Returns a `Verdict` via
/// return-data (as `verify` does) and emits a reproducible `PolicyDecision`
/// stamped with the policy version. Never reverts on a negative result.
#[derive(Accounts)]
pub struct VerifyPolicy<'info> {
    pub policy: Account<'info, Policy>,

    /// CHECK: the aegis attestation PDA for (policy.issuer, subject,
    /// policy.schema_id); re-derived and owner-checked inside `evaluate`.
    pub attestation: UncheckedAccount<'info>,
}

pub fn handle_verify_policy(ctx: Context<VerifyPolicy>, subject: Pubkey) -> Result<()> {
    let policy = &ctx.accounts.policy;
    let predicate = VerifyPredicate::Present {
        issuer: policy.issuer,
        subject,
        schema_id: policy.schema_id,
    };
    let verdict = evaluate(
        &ctx.accounts.attestation,
        &predicate,
        policy.issuer,
        subject,
        policy.schema_id,
        policy.freshness_secs,
    );

    emit!(PolicyDecision {
        policy: policy.key(),
        policy_version: policy.version,
        subject,
        ok: verdict.ok,
        reason_code: verdict.reason_code,
    });
    emit_verdict(&verdict)
}
