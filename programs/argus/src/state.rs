use anchor_lang::prelude::*;

/// Per-(mint, source-owner) daily gift velocity ledger.
///
/// Deliberately non-closable: closing and reopening would reset the daily
/// cap, so the locked rent is the anti-reset bond.
#[account]
#[derive(InitSpace)]
pub struct GiftLedger {
    pub day: u32,
    pub gifted_today: u64,
    pub bump: u8,
}
