use anchor_lang::prelude::*;

// ── Lifecycle ──────────────────────────────────────────────────────────────

#[event]
pub struct TransferGuardInitialized {
    pub mint: Pubkey,
    pub merchant: Pubkey,
    pub authority: Pubkey,
}

#[event]
pub struct PolicyConfigured {
    pub mint: Pubkey,
    pub flags: u16,
    pub daily_gift_cap: u64,
    pub per_tx_cap: u64,
    pub max_wallet_balance: u64,
    pub transfers_per_day_cap: u16,
    pub cooldown_secs: u32,
    pub attestation_schema: u64,
    pub policy_epoch: u64,
}

/// A subject's eligibility was (re)computed via aegis `verify` and cached.
#[event]
pub struct EligibilityRefreshed {
    pub mint: Pubkey,
    pub subject: Pubkey,
    pub verdicts: u32,
    pub expires_at: i64,
}

/// A subject's cached capability was force-invalidated by the guard authority.
#[event]
pub struct CapabilityInvalidated {
    pub mint: Pubkey,
    pub subject: Pubkey,
}

#[event]
pub struct GuardPausedSet {
    pub mint: Pubkey,
    pub paused: bool,
}

#[event]
pub struct GuardAuthorityProposed {
    pub mint: Pubkey,
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct GuardAuthorityChanged {
    pub mint: Pubkey,
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct ListEntryChanged {
    pub mint: Pubkey,
    pub target: Pubkey,
    pub added: bool,
}

#[event]
pub struct WalletStateOpened {
    pub mint: Pubkey,
    pub owner: Pubkey,
}

// ── Governance (spec 10) ─────────────────────────────────────────────────────

#[event]
pub struct GovernanceInitialized {
    pub mint: Pubkey,
    pub role_admin: Pubkey,
    pub genesis_policy_hash: [u8; 32],
    pub timelock_secs: i64,
}

#[event]
pub struct PolicyProposed {
    pub mint: Pubkey,
    pub policy_hash: [u8; 32],
    pub author: Pubkey,
}

#[event]
pub struct PolicyApproved {
    pub mint: Pubkey,
    pub policy_hash: [u8; 32],
    pub approver: Pubkey,
    pub activate_after: i64,
}

#[event]
pub struct PolicyActivated {
    pub mint: Pubkey,
    pub old_hash: [u8; 32],
    pub new_hash: [u8; 32],
    pub policy_epoch: u64,
    /// True when this activation was an expedited rollback to a prior version.
    pub rollback: bool,
}

#[event]
pub struct PolicyPinned {
    pub mint: Pubkey,
    pub policy_hash: [u8; 32],
}

#[event]
pub struct RoleChangeProposed {
    pub mint: Pubkey,
    pub role: u8,
    pub authority: Pubkey,
    pub apply_after: i64,
}

#[event]
pub struct RoleChanged {
    pub mint: Pubkey,
    pub role: u8,
    pub old: Pubkey,
    pub new: Pubkey,
}

// ── Per-transfer decision (spec §10) ─────────────────────────────────────────

/// One event per `execute` call, carrying a stable canonical reason code
/// (`crate::constants::reason`) plus the exact deciding policy (`policy_epoch` +
/// `active_policy_hash`, spec 10 §4.5). The complete allow/deny audit trail —
/// an indexer folds these into period `StatementCommitment` roots.
#[event]
pub struct TransferDecision {
    pub mint: Pubkey,
    pub source_owner: Pubkey,
    pub destination_owner: Pubkey,
    pub amount: u64,
    pub allowed: bool,
    pub reason: u16,
    /// Monotonic policy epoch in force at decision time.
    pub policy_epoch: u64,
    /// Hash of the active governed policy (`[0;32]` for a free-tier mint).
    pub active_policy_hash: [u8; 32],
}

/// A period's decision statement was anchored on-chain (spec 10 §4.5).
#[event]
pub struct StatementAnchored {
    pub mint: Pubkey,
    pub period: u64,
    pub merkle_root: [u8; 32],
    pub decision_count: u64,
    pub reporter: Pubkey,
}
