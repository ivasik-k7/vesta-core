use anchor_lang::prelude::*;

/// Account layout version (Track B convention) stamped into GuardConfig /
/// EligibilityCapability; readers fail closed on an unknown version.
pub const STATE_VERSION: u8 = 1;

/// Per-mint policy account (spec §2.1).
#[constant]
pub const GUARD_SEED: &[u8] = b"guard";

/// Per-(mint, source-owner) velocity counters (spec §2.2).
#[constant]
pub const WALLET_STATE_SEED: &[u8] = b"wstate";

/// Allow/deny list membership marker (spec §2.4).
#[constant]
pub const LIST_ENTRY_SEED: &[u8] = b"entry";

/// Interface-mandated seed for the ExtraAccountMetaList PDA.
#[constant]
pub const EXTRA_ACCOUNT_METAS_SEED: &[u8] = b"extra-account-metas";

/// Per-(mint, subject) cached eligibility verdict (spec 09 §4.1).
#[constant]
pub const CAP_SEED: &[u8] = b"cap";

// ── Governance (spec 10) ─────────────────────────────────────────────────────

/// Immutable, content-addressed policy version (spec 10 §5): `["pver", mint, hash]`.
#[constant]
pub const POLICY_VERSION_SEED: &[u8] = b"pver";

/// Per-mint active-policy pointer + timelock state (spec 10 §5): `["active", mint]`.
#[constant]
pub const POLICY_POINTER_SEED: &[u8] = b"active";

/// Per-mint separation-of-duties role registry (spec 10 §5): `["roles", mint]`.
#[constant]
pub const ROLES_SEED: &[u8] = b"roles";

/// Per-(mint, period) decision-statement anchor (spec 10 §5): `["statement", mint, period]`.
#[constant]
pub const STATEMENT_SEED: &[u8] = b"statement";

/// Per-mint trust-triangle anchor (spec 10 §5): `["trust", mint]`.
#[constant]
pub const TRUST_SEED: &[u8] = b"trust";

/// Default trust-triangle grace window, seconds — a failing accreditation
/// streak must persist this long before auto-degrade bites (absorbs a transient
/// aegis outage). Configurable per mint on the `TrustAnchor`.
pub const DEFAULT_TRUST_GRACE_SECS: i64 = 3_600;

/// Trust-triangle degrade postures stored in `GuardConfig.degrade_mode` /
/// `TrustAnchor.degrade_target` (spec 10 §4.3). Any non-`NORMAL` mode blocks
/// peer gifts while leaving redemption (treasury) and clawback (delegate) open,
/// so degradation never strands holder assets.
pub mod degrade {
    pub const NORMAL: u8 = 0;
    pub const REDEMPTION_ONLY: u8 = 1;
    pub const FROZEN: u8 = 2;

    /// Modes accepted as a `degrade_target` (must be an actual degraded state).
    pub fn is_valid_target(mode: u8) -> bool {
        mode == REDEMPTION_ONLY || mode == FROZEN
    }
}

/// Default governance timelock, seconds, seeded when a mint adopts governance.
/// A change is `propose → approve → wait(timelock) → activate`; a compromised
/// approver alone still cannot rush a rule change past the delay.
pub const DEFAULT_GOVERNANCE_TIMELOCK_SECS: i64 = 86_400;

/// Upper bound on the configurable timelock (365 days) — a sanity clamp so a
/// fat-fingered value can't lock a mint's policy effectively forever.
pub const MAX_GOVERNANCE_TIMELOCK_SECS: i64 = 365 * 86_400;

/// How long a minted `EligibilityCapability` stays valid, seconds. A shorter
/// window means fresher revocation at the cost of more refreshes; a config
/// `policy_epoch` bump invalidates all capabilities immediately regardless.
pub const CAPABILITY_TTL_SECS: i64 = 86_400;

/// `EligibilityCapability.verdicts` bit for the guard's REQUIRE_ATTESTATION
/// predicate (the destination holds a valid aegis credential of the configured
/// schema from the trusted issuer).
pub const PREDICATE_ATTESTATION_BIT: u32 = 1 << 0;

/// Default daily gift velocity cap seeded at guard init, raw units
/// (= 500.00 pts at issue). Retunable via `configure_policy` thereafter.
#[constant]
pub const DEFAULT_DAILY_GIFT_CAP_RAW: u64 = 50_000;

pub const SECONDS_PER_DAY: i64 = 86_400;

/// vesta_core program id — argus deliberately does NOT link the vesta-core
/// crate (a crate dependency would poison the workspace build via
/// no-entrypoint feature unification); cross-checked by an integration test.
pub const VESTA_CORE_ID: Pubkey = pubkey!("gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz");

/// Anchor discriminator of vesta_core's Merchant account
/// (sha256("account:Merchant")[..8]); equality is asserted in tests.
pub const MERCHANT_DISCRIMINATOR: [u8; 8] = [71, 235, 30, 40, 231, 21, 32, 64];

/// aegis program id — the canonical attestation issuer argus composes with
/// (spec §7, §13). argus does NOT link the aegis crate for the same
/// feature-unification reason; the Attestation layout below is verified by an
/// integration test. Rotated via `anchor keys sync` at deploy time.
pub const AEGIS_ID: Pubkey = pubkey!("AcCdMQC1rj4KukjhFzf4S8metEAXpnt9gzvMThsu15e1");

/// Policy bitset stored in `GuardConfig.flags` (spec §2.1, §5).
pub mod flags {
    /// Reject destinations owned by a program (best-effort; spec §5 rule 5).
    pub const BLOCK_PROGRAM_OWNED: u16 = 1 << 0;
    /// Peer transfers require the destination to be in the allow list.
    pub const ALLOWLIST_ONLY: u16 = 1 << 1;
    /// Peer transfers are rejected if the destination is in the deny list.
    pub const DENYLIST: u16 = 1 << 2;
    /// Peer transfers require a valid aegis attestation on the destination.
    pub const REQUIRE_ATTESTATION: u16 = 1 << 3;
    /// Hard-disable peer transfers regardless of caps (issuer/treasury only).
    pub const GIFTING_DISABLED: u16 = 1 << 4;

    /// Bits with defined meaning; anything else is rejected at configure time.
    pub const KNOWN: u16 =
        BLOCK_PROGRAM_OWNED | ALLOWLIST_ONLY | DENYLIST | REQUIRE_ATTESTATION | GIFTING_DISABLED;
}

/// Stable reason codes emitted with every `execute` decision (spec §10).
pub mod reason {
    pub const ISSUER_FLOW: u16 = 0;
    pub const TREASURY_FLOW: u16 = 1;
    pub const GIFT: u16 = 2;
    pub const NOOP: u16 = 3;
    pub const MINT_PAUSED: u16 = 10;
    pub const GIFTING_DISABLED: u16 = 11;
    pub const PROGRAM_OWNED_DEST: u16 = 12;
    pub const NOT_ALLOWLISTED: u16 = 13;
    pub const DENY_LISTED: u16 = 14;
    pub const ATTESTATION_FAILED: u16 = 15;
    pub const PER_TX_EXCEEDED: u16 = 16;
    pub const BALANCE_CAP: u16 = 17;
    pub const COOLDOWN: u16 = 18;
    pub const TRANSFER_COUNT: u16 = 19;
    pub const DAILY_CAP: u16 = 20;
    pub const CONFIG_ERROR: u16 = 21;
    pub const STATE_MISSING: u16 = 22;
    /// Destination has no fresh eligibility capability (client must refresh).
    pub const ELIGIBILITY_STALE: u16 = 23;
    /// Peer transfers blocked by the trust-triangle degrade posture (spec 10 §4.3).
    pub const TRUST_DEGRADED: u16 = 24;
}
