//! Decision statements (spec 10, phase 2).
//!
//! Every `execute` emits a `TransferDecision` carrying a canonical reason code
//! and the exact deciding policy (`policy_epoch` + `active_policy_hash`). An
//! off-chain indexer folds a period's decisions into a Merkle tree; the Reporter
//! role anchors that root here as a `StatementCommitment`. `decision_count`
//! makes the statement *provably complete* — an omitted decision changes both
//! the count and the root, so cherry-picking is detectable.

use anchor_lang::prelude::*;

use crate::{
    constants::{entitlement, LICENSE_SEED, ROLES_SEED, STATEMENT_SEED, STATE_VERSION},
    error::GuardError,
    events::StatementAnchored,
    state::{LicenseState, Role, RoleRegistry, StatementCommitment},
};

#[derive(Accounts)]
#[instruction(period: u64)]
pub struct AnchorStatement<'info> {
    #[account(mut)]
    pub reporter: Signer<'info>,

    /// CHECK: the mint this statement covers — PDA seed only.
    pub mint: UncheckedAccount<'info>,

    #[account(
        seeds = [ROLES_SEED, mint.key().as_ref()],
        bump = role_registry.bump,
    )]
    pub role_registry: Account<'info, RoleRegistry>,

    /// Statements are a premium feature — a live license with the STATEMENTS
    /// entitlement is required (spec 10 §4.7, the revenue wedge).
    #[account(
        seeds = [LICENSE_SEED, mint.key().as_ref()],
        bump = license.bump,
    )]
    pub license: Account<'info, LicenseState>,

    #[account(
        init,
        payer = reporter,
        space = 8 + StatementCommitment::INIT_SPACE,
        seeds = [STATEMENT_SEED, mint.key().as_ref(), &period.to_le_bytes()],
        bump,
    )]
    pub statement: Account<'info, StatementCommitment>,

    pub system_program: Program<'info, System>,
}

pub fn handle_anchor_statement(
    ctx: Context<AnchorStatement>,
    period: u64,
    merkle_root: [u8; 32],
    decision_count: u64,
) -> Result<()> {
    ctx.accounts
        .role_registry
        .require(Role::Reporter, ctx.accounts.reporter.key())?;

    let now = Clock::get()?.unix_timestamp;
    require!(
        ctx.accounts.license.grants(entitlement::STATEMENTS, now),
        GuardError::LicenseNotEntitled
    );
    let mint = ctx.accounts.mint.key();

    let statement = &mut ctx.accounts.statement;
    statement.version = STATE_VERSION;
    statement.mint = mint;
    statement.period = period;
    statement.merkle_root = merkle_root;
    statement.decision_count = decision_count;
    statement.reporter = ctx.accounts.reporter.key();
    statement.anchored_at = now;
    statement.bump = ctx.bumps.statement;

    emit!(StatementAnchored {
        mint,
        period,
        merkle_root,
        decision_count,
        reporter: ctx.accounts.reporter.key(),
    });
    Ok(())
}
