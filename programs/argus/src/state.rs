use anchor_lang::prelude::*;

/// Per-mint transfer policy — the tunable heart of the guard (spec §2.1).
///
/// One account, read once in `execute`, holds the whole policy. The guard
/// authority (the merchant PDA) may retune every field except the attestation
/// issuer/program, which are baked into the ExtraAccountMetaList at init and
/// therefore fixed for the life of the guard (spec §7, §16).
#[account]
#[derive(InitSpace)]
pub struct GuardConfig {
    /// The hooked mint this config governs.
    pub mint: Pubkey,
    /// Who may retune policy — the vesta_core Merchant PDA.
    pub authority: Pubkey,
    /// Two-step authority rotation target.
    pub pending_authority: Option<Pubkey>,
    /// Always-allowed destination (rule 2): the merchant treasury ATA.
    pub treasury: Pubkey,
    /// aegis issuer whose attestations this guard trusts (spec §7). Baked into
    /// the meta list at init; `Pubkey::default()` when attestation is unused.
    pub attestation_issuer: Pubkey,
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
    /// Required attestation schema id (meaningful only with REQUIRE_ATTESTATION).
    pub attestation_schema: u16,
    /// Required attestation value bitmask; allow iff `value & mask != 0`.
    pub attestation_mask: u64,
    pub bump: u8,
}

/// Per-(mint, source-owner) velocity counters (spec §2.2). Supersedes v1's
/// `GiftLedger`. Deliberately non-closable: closing and reopening would reset
/// the daily counters, so the locked rent is the anti-reset bond.
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
/// program == membership; the account carries no payload beyond identity, so
/// `execute` never scans — a single PDA derivation answers the question.
#[account]
#[derive(InitSpace)]
pub struct PolicyListEntry {
    pub mint: Pubkey,
    pub target: Pubkey,
    pub bump: u8,
}
