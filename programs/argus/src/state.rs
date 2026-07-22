use anchor_lang::prelude::*;

/// Per-mint transfer policy — the tunable heart of the guard (spec §2.1).
///
/// One account, read once in `execute`. The guard authority (the merchant PDA)
/// retunes fields via `configure_policy`; the trusted aegis issuer/program are
/// bound at init. Eligibility is no longer read from aegis by byte offset —
/// `execute` reads a cached `EligibilityCapability` (spec 09) minted off the hot
/// path by `refresh_eligibility`, which consumes aegis's `verify` interface.
#[account]
#[derive(InitSpace)]
pub struct GuardConfig {
    /// Layout version (Track B convention).
    pub version: u8,
    /// The hooked mint this config governs.
    pub mint: Pubkey,
    /// Who may retune policy — the vesta_core Merchant PDA (rotatable two-step).
    pub authority: Pubkey,
    /// Two-step authority rotation target.
    pub pending_authority: Option<Pubkey>,
    /// Always-allowed destination (rule 2): the merchant treasury ATA.
    pub treasury: Pubkey,
    /// aegis deployment this guard trusts (the `verify` program).
    pub aegis_program: Pubkey,
    /// aegis issuer whose credentials this guard trusts (spec §7).
    pub attestation_issuer: Pubkey,
    /// aegis `Policy` this guard enforces (spec 07). When set, `refresh_eligibility`
    /// consumes `verify_policy` — so the compliance rule (jurisdiction, schema,
    /// freshness) lives in aegis as data, editable with NO argus redeploy. When
    /// `default()`, the legacy single-credential `Present` check is used instead.
    pub policy: Pubkey,
    /// Bumped on any policy change; stamped into capabilities so a config change
    /// invalidates stale eligibility regardless of TTL (spec 09 §4.4).
    pub policy_epoch: u64,
    /// Per-mint peer-transfer freeze (rules 1–2 stay open — clawback/refunds).
    pub paused: bool,
    /// Policy bitset — see `crate::constants::flags`.
    pub flags: u16,
    /// Raw units a wallet may send per UTC day (0 = gifting off).
    pub daily_gift_cap: u64,
    /// Max raw units in a single peer transfer (0 = no per-tx limit).
    pub per_tx_cap: u64,
    /// Reject transfers that would push the destination over this (0 = off).
    pub max_wallet_balance: u64,
    /// Max peer transfers a wallet may make per UTC day (0 = no count limit).
    pub transfers_per_day_cap: u16,
    /// Minimum seconds between a wallet's peer transfers (0 = no cooldown).
    pub cooldown_secs: u32,
    /// Required aegis schema id (meaningful only with REQUIRE_ATTESTATION).
    pub attestation_schema: u64,
    /// Per-guard eligibility-capability TTL, seconds (0 = protocol default). A
    /// strict mint sets this low to shrink the aegis-revocation-latency window;
    /// `invalidate_capability` closes it immediately for a known revocation.
    pub capability_ttl_secs: i64,
    pub bump: u8,
}

/// Per-(mint, source-owner) velocity counters (spec §2.2). Deliberately
/// non-closable: closing and reopening would reset the daily counters, so the
/// locked rent is the anti-reset bond.
#[account]
#[derive(InitSpace)]
pub struct WalletPolicyState {
    /// UTC day the counters below belong to.
    pub day: u32,
    /// Raw units sent as peer transfers today.
    pub sent_today: u64,
    /// Peer-transfer count today.
    pub transfers_today: u16,
    /// Unix ts of the wallet's last peer transfer (cooldown reference).
    pub last_transfer_at: i64,
    pub bump: u8,
}

/// Allow/deny list membership marker (spec §2.4). Existence under the argus
/// program == membership; the account carries no payload beyond identity.
#[account]
#[derive(InitSpace)]
pub struct PolicyListEntry {
    pub mint: Pubkey,
    pub target: Pubkey,
    pub bump: u8,
}

/// Cached eligibility verdict for a subject (spec 09 §4.1). `refresh_eligibility`
/// pays aegis's `verify` CPI once, off the transfer path, and stamps the result
/// here; `execute` then does an O(1) freshness + bitmap read with NO CPI. A
/// missing/stale capability fails the transfer closed (client must refresh).
#[account]
#[derive(InitSpace)]
pub struct EligibilityCapability {
    pub version: u8,
    pub mint: Pubkey,
    pub subject: Pubkey,
    /// Bit `i` set = predicate `i` satisfied (see `crate::constants` bits).
    pub verdicts: u32,
    /// aegis deployment consulted when this capability was minted.
    pub aegis_program: Pubkey,
    /// `GuardConfig.policy_epoch` at mint time; must still match to be valid.
    pub policy_epoch: u64,
    pub issued_at: i64,
    /// Unix expiry; the capability is stale once `now >= expires_at`.
    pub expires_at: i64,
    pub bump: u8,
}
