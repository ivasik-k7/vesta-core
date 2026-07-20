use anchor_lang::prelude::*;

use crate::constants::MAX_NAME_LEN;

/// An attestation authority. Issues credentials for subjects that downstream
/// programs (argus, vesta_core campaigns) gate on.
///
/// Two keys, by design (hot/cold separation): `authority` is the cold admin —
/// it rotates authority, pauses, and sets the operator. `operator` is an
/// optional hot signing key that may issue / update / revoke / close
/// attestations but can never touch admin state. High-volume issuers keep the
/// authority offline and rotate the operator freely.
#[account]
#[derive(InitSpace)]
pub struct Issuer {
    /// Per-(authority, id) — a wallet may run many issuers.
    pub id: u64,
    pub authority: Pubkey,
    pub pending_authority: Option<Pubkey>,
    /// Optional hot signing key for day-to-day issuance (None = authority only).
    pub operator: Option<Pubkey>,
    #[max_len(MAX_NAME_LEN)]
    pub name: String,
    /// Lifetime count of attestations issued (monotonic; audit metric).
    pub issued: u64,
    /// When true, no new attestations may be issued or updated.
    pub paused: bool,
    pub bump: u8,
}

impl Issuer {
    /// A signer authorized for issuance ops (authority or the hot operator).
    pub fn is_signer_authorized(&self, signer: &Pubkey) -> bool {
        *signer == self.authority || self.operator == Some(*signer)
    }
}

/// A signed credential binding a subject wallet to a value under a schema.
///
/// FIELD ORDER IS AN ABI. argus reads this account by fixed byte offset
/// (`argus::constants::attestation_offset`) rather than linking the aegis
/// crate. The layout below MUST stay: disc(8) · issuer(32) · subject(32) ·
/// schema(u16) · value(u64) · issued_at(i64) · valid_from(i64) · expires_at(i64)
/// · revoked(bool) · bump(u8). Reordering breaks every guard that gates on it.
#[account]
#[derive(InitSpace)]
pub struct Attestation {
    pub issuer: Pubkey,
    pub subject: Pubkey,
    pub schema: u16,
    /// Schema-defined payload — typically a bitmask (regions, tiers).
    pub value: u64,
    pub issued_at: i64,
    /// Unix "not-before"; 0 means valid immediately. Enables pre-issuance.
    pub valid_from: i64,
    /// Unix expiry; 0 means never expires.
    pub expires_at: i64,
    pub revoked: bool,
    pub bump: u8,
}
