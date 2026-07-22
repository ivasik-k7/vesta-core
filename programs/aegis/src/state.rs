use anchor_lang::prelude::*;

use crate::constants::{MAX_NAME_LEN, MAX_STANDARD_URI_LEN, STATE_VERSION};

/// An attestation authority. Issues credentials for subjects that downstream
/// programs (argus, vesta_core campaigns) gate on via the `verify` interface.
///
/// Two keys, by design (hot/cold separation): `authority` is the cold admin —
/// it rotates authority, pauses, and sets the operator. `operator` is an
/// optional hot signing key that may issue / update / revoke / close
/// attestations but can never touch admin state.
#[account]
#[derive(InitSpace)]
pub struct Issuer {
    /// Layout version (Track B convention) — first field after the discriminator.
    pub version: u8,
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

/// A typed, versioned credential schema (spec 06). Schemas are shapes, not
/// instances — they carry NO subject data. `content_hash` anchors the off-chain
/// schema document (Arweave/IPFS); `sas_schema` optionally aliases a Solana
/// Attestation Service schema so the two namespaces reconcile.
#[account]
#[derive(InitSpace)]
pub struct Schema {
    pub version: u8,
    pub registrar: Pubkey,
    pub id: u64,
    /// Content hash of the off-chain schema document.
    pub content_hash: [u8; 32],
    #[max_len(MAX_STANDARD_URI_LEN)]
    pub standard_uri: String,
    /// Optional alias to a SAS schema account.
    pub sas_schema: Option<Pubkey>,
    pub deprecated: bool,
    pub successor: Option<Pubkey>,
    pub bump: u8,
}

/// A trust root (spec 08): an entity a verifier chooses to trust. Issuers it
/// accredits inherit that trust, so a verifier pins ONE root instead of a
/// hand-maintained allowlist of issuer keys. Declaration is permissionless;
/// trust is conferred by whichever verifier pins the root.
#[account]
#[derive(InitSpace)]
pub struct TrustRoot {
    pub version: u8,
    pub authority: Pubkey,
    #[max_len(MAX_NAME_LEN)]
    pub name: String,
    pub active: bool,
    pub bump: u8,
}

/// Accreditation status. `Revoked` is terminal (de-trusts the issuer instantly).
pub mod accreditation_status {
    pub const ACTIVE: u8 = 0;
    pub const REVOKED: u8 = 1;
}

/// A direct accreditation edge (spec 08 phase 1a): `root` vouches for
/// `subject_issuer` to issue credentials of `permitted_schemas` in
/// `jurisdiction`. `AccreditedBy(root)` verification walks these edges — for now
/// a single hop (recursive root→sector→issuer chains are a later sub-phase).
#[account]
#[derive(InitSpace)]
pub struct Accreditation {
    pub version: u8,
    /// The trust-root authority that granted this accreditation.
    pub root: Pubkey,
    /// The aegis `Issuer` PDA being accredited.
    pub subject_issuer: Pubkey,
    pub tier: u8,
    /// Schemas this issuer is accredited for; `count == 0` means all schemas.
    pub permitted_schemas: [u64; crate::constants::MAX_PERMITTED_SCHEMAS],
    pub permitted_count: u8,
    pub jurisdiction: u16,
    pub status: u8,
    pub issued_at: i64,
    /// Unix expiry; 0 = never.
    pub expires_at: i64,
    pub bump: u8,
}

impl Accreditation {
    /// Live = active status and within the validity window at `now`.
    pub fn is_live(&self, now: i64) -> bool {
        self.status == accreditation_status::ACTIVE
            && (self.expires_at == 0 || now < self.expires_at)
    }

    /// Whether this accreditation covers `schema_id` (empty list = all).
    pub fn permits(&self, schema_id: u64) -> bool {
        let count = usize::from(self.permitted_count);
        count == 0
            || self.permitted_schemas[..count.min(self.permitted_schemas.len())]
                .contains(&schema_id)
    }
}

/// A named, versioned, jurisdiction-tagged verifier policy (spec 07). A verifier
/// references a policy by name instead of inlining checks; `verify_policy`
/// returns a `Verdict` and emits an audit event stamped with `version`, so an
/// accept/reject decision is reproducible against the policy live at the time.
#[account]
#[derive(InitSpace)]
pub struct Policy {
    pub version: u8,
    pub authority: Pubkey,
    pub id: u64,
    /// Jurisdiction code (0 = global) — the same credential can pass one
    /// jurisdiction's policy and fail another, deterministically.
    pub jurisdiction: u16,
    /// Required credential issuer.
    pub issuer: Pubkey,
    /// Required credential schema.
    pub schema_id: u64,
    /// Max credential age in seconds (0 = no freshness requirement).
    pub freshness_secs: i64,
    pub deprecated: bool,
    pub successor: Option<Pubkey>,
    pub bump: u8,
}

/// Attestation lifecycle status. `Revoked` and `Erased` are terminal.
pub mod attestation_status {
    pub const ACTIVE: u8 = 0;
    /// Credential withdrawn by the issuer (terminal).
    pub const REVOKED: u8 = 1;
    /// Off-chain PII + salt destroyed (GDPR cryptographic erasure, terminal).
    pub const ERASED: u8 = 2;
}

/// A privacy-preserving credential (spec 06): the chain holds only a hiding +
/// binding COMMITMENT and a per-attribute Merkle root, never plaintext claims.
/// The real (W3C-VC-shaped) credential lives off-chain with the holder; the
/// chain is the integrity / freshness / revocation anchor.
///
/// Multi-credential: keyed by (issuer, subject, schema_id), a subject holds
/// many independent credentials from one issuer. This replaces the v1 public
/// `value: u64` bitmask that argus read by fixed offset — consumers now go
/// through the `verify` interface, not raw offsets.
#[account]
#[derive(InitSpace)]
pub struct Attestation {
    pub version: u8,
    pub issuer: Pubkey,
    /// Holder-binding handle (not necessarily the payment wallet).
    pub subject: Pubkey,
    pub schema_id: u64,
    /// Commitment `H(claims ‖ holder_binding ‖ salt)` (sha256 in phase 1;
    /// Poseidon is the wave-2 target for ZK-circuit field compatibility).
    pub commitment: [u8; 32],
    /// Per-attribute Merkle root, for single-attribute selective disclosure.
    pub attr_root: [u8; 32],
    pub issued_at: i64,
    /// Unix "not-before"; 0 = valid immediately.
    pub valid_from: i64,
    /// Unix expiry; 0 = never.
    pub expires_at: i64,
    /// `attestation_status::*`.
    pub status: u8,
    pub bump: u8,
}

impl Attestation {
    pub const VERSION: u8 = STATE_VERSION;

    /// Live = active status and within the validity window at `now`.
    pub fn is_live(&self, now: i64) -> bool {
        self.status == attestation_status::ACTIVE
            && (self.valid_from == 0 || now >= self.valid_from)
            && (self.expires_at == 0 || now < self.expires_at)
    }
}
