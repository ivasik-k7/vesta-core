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
    pub attestation_schema: u16,
    pub attestation_mask: u64,
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

// ── Per-transfer decision (spec §10) ─────────────────────────────────────────

/// One event per `execute` call, carrying a stable reason code
/// (`crate::constants::reason`). The complete allow/deny audit trail.
#[event]
pub struct TransferDecision {
    pub mint: Pubkey,
    pub source_owner: Pubkey,
    pub destination_owner: Pubkey,
    pub amount: u64,
    pub allowed: bool,
    pub reason: u16,
}
