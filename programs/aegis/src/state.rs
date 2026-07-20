use anchor_lang::prelude::*;

use crate::constants::MAX_NAME_LEN;

/// An attestation authority. Issues credentials for subjects that downstream
/// programs (argus, vesta_core campaigns) gate on.
#[account]
#[derive(InitSpace)]
pub struct Issuer {
    pub authority: Pubkey,
    pub pending_authority: Option<Pubkey>,
    #[max_len(MAX_NAME_LEN)]
    pub name: String,
    /// Lifetime count of attestations issued (monotonic; audit metric).
    pub issued: u64,
    /// When true, no new attestations may be issued or updated.
    pub paused: bool,
    pub bump: u8,
}

/// A signed credential binding a subject wallet to a value under a schema.
///
/// FIELD ORDER IS AN ABI. argus reads this account by fixed byte offset
/// (`argus::constants::attestation_offset`) rather than linking the aegis
/// crate. The layout below MUST stay: disc(8) · issuer(32) · subject(32) ·
/// schema(u16) · value(u64) · issued_at(i64) · expires_at(i64) · revoked(bool)
/// · bump(u8). Reordering breaks every guard that gates on attestation.
#[account]
#[derive(InitSpace)]
pub struct Attestation {
    pub issuer: Pubkey,
    pub subject: Pubkey,
    pub schema: u16,
    /// Schema-defined payload — typically a bitmask (regions, tiers).
    pub value: u64,
    pub issued_at: i64,
    /// Unix expiry; 0 means never expires.
    pub expires_at: i64,
    pub revoked: bool,
    pub bump: u8,
}
