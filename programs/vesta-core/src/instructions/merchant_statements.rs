//! Merchant decision statements (spec 13 §4.4, phase 3).
//!
//! A tamper-evident, provably-complete audit ledger for the merchant economy —
//! the analogue of argus's `StatementCommitment`, for *economic* decisions
//! (earns / redemptions / clawbacks) rather than transfer decisions. An
//! off-chain indexer materializes a period's decisions into a Merkle tree; the
//! merchant owner anchors its root + a `decision_count` completeness witness.
//! Anchored once per period (immutable/append-only), so an omission is
//! detectable (it changes both the count and the root).

use anchor_lang::prelude::*;

use crate::{
    constants::{MERCHANT_SEED, MERCHANT_STATEMENT_SEED, STATE_VERSION},
    error::VestaError,
    events::MerchantStatementAnchored,
    state::{Merchant, MerchantStatement},
};

#[derive(Accounts)]
#[instruction(period: u64)]
pub struct AnchorMerchantStatement<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        init,
        payer = authority,
        space = 8 + MerchantStatement::INIT_SPACE,
        seeds = [MERCHANT_STATEMENT_SEED, merchant.key().as_ref(), &period.to_le_bytes()],
        bump,
    )]
    pub statement: Account<'info, MerchantStatement>,

    pub system_program: Program<'info, System>,
}

pub fn handle_anchor_merchant_statement(
    ctx: Context<AnchorMerchantStatement>,
    period: u64,
    merkle_root: [u8; 32],
    decision_count: u64,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let merchant = ctx.accounts.merchant.key();
    let reporter = ctx.accounts.authority.key();

    let s = &mut ctx.accounts.statement;
    s.version = STATE_VERSION;
    s.merchant = merchant;
    s.period = period;
    s.merkle_root = merkle_root;
    s.decision_count = decision_count;
    s.reporter = reporter;
    s.anchored_at = now;
    s.bump = ctx.bumps.statement;

    emit!(MerchantStatementAnchored {
        merchant,
        period,
        merkle_root,
        decision_count,
        reporter,
    });
    Ok(())
}
