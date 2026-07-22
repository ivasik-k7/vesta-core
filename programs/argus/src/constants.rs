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
}
