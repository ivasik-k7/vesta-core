use anchor_lang::prelude::*;

/// Max operational delegate keys a merchant may authorize (hot POS/back-office
/// keys distinct from the cold owner authority).
pub const MAX_OPERATORS: usize = 4;

/// Global protocol config. Singleton PDA.
#[account]
#[derive(InitSpace)]
pub struct Config {
    pub admin: Pubkey,
    pub pending_admin: Option<Pubkey>,
    pub paused: bool,
    pub bump: u8,
}

/// One record per (authority, id) — a wallet owns MANY merchants (multi-brand /
/// multi-location). `id` + `authority` are the PDA seeds. Field order is an ABI:
/// argus reads `id`/`authority`/`point_mint`/`treasury` by fixed offset, so the
/// fixed-size prefix below must not be reordered.
///
/// Enterprise surface: a cold `authority` (owner) plus up to `MAX_OPERATORS`
/// hot delegate keys that may run day-to-day operations (earn, campaigns,
/// achievements) without the owner key; a merchant-level pause; an admin-set
/// `verified` trust flag; a display category + metadata URI; lifetime stats.
#[account]
#[derive(InitSpace)]
pub struct Merchant {
    pub id: u64,
    pub authority: Pubkey,
    pub point_mint: Pubkey,
    pub treasury: Pubkey,
    #[max_len(32)]
    pub name: String,
    pub decay_rate_bps: i16,
    pub base_earn_rate: u64,
    pub lifetime_points_issued: u128,
    pub customer_count: u64,
    pub joined_alliance: Option<Pubkey>,
    // ── enterprise fields ──────────────────────────────────────────────────
    /// Hot operational keys (earn/campaigns/achievements). Owner is implicit.
    pub operators: [Pubkey; MAX_OPERATORS],
    pub operator_count: u8,
    /// Merchant-scoped freeze of earn/redeem, independent of the global pause.
    pub paused: bool,
    /// Protocol-admin-set trust badge (e.g. KYB-verified brand).
    pub verified: bool,
    /// Display category (free-form enum; UI maps to a label).
    pub category: u8,
    /// Off-chain profile JSON (logo, links, description).
    #[max_len(128)]
    pub metadata_uri: String,
    pub lifetime_redemptions: u64,
    pub badges_issued: u64,
    // ── clawback (compliance) surface ───────────────────────────────────────
    /// Lifetime raw points reclaimed via clawback (audit metric).
    pub lifetime_clawed_back: u128,
    pub clawback_count: u64,
    /// Max raw points clawable per UTC day; 0 = unlimited. Defense-in-depth cap
    /// on the owner-only clawback action (owner key compromise).
    pub clawback_daily_cap_raw: u64,
    pub clawed_today: u64,
    pub clawback_day: u32,
    pub bump: u8,
    pub mint_bump: u8,
}

impl Merchant {
    /// Owner authority or any authorized operator.
    pub fn can_operate(&self, signer: &Pubkey) -> bool {
        *signer == self.authority
            || self.operators[..usize::from(self.operator_count)].contains(signer)
    }
}

/// Per merchant-customer pair.
#[account]
#[derive(InitSpace)]
pub struct CustomerProfile {
    pub wallet: Pubkey,
    pub merchant: Pubkey,
    pub streak_days: u16,
    pub last_visit_day: u32,
    pub lifetime_earned: u64,
    pub lifetime_redemptions: u32,
    pub tier: u8,
    /// Total qualifying spend (base minor units) seen across earns.
    pub lifetime_spend_base: u64,
    /// Count of quest-style campaigns completed.
    pub campaigns_completed: u16,
    /// Lifetime raw points reclaimed from this customer via clawback.
    pub lifetime_clawed_back: u64,
    pub clawback_count: u32,
    pub bump: u8,
}

/// Redemption catalog entry. `merchant` is deliberately the first field so
/// getProgramAccounts can memcmp on it at offset 8.
#[account]
#[derive(InitSpace)]
pub struct Offer {
    pub merchant: Pubkey,
    pub id: u64,
    /// Denominated in UI points ×10² (post-decay purchasing power).
    pub price_points: u64,
    pub supply_remaining: u32,
    pub active: bool,
    pub bump: u8,
}

/// Voucher for a redemption; indexed by the profile's on-chain counter.
#[account]
#[derive(InitSpace)]
pub struct Receipt {
    pub offer: Pubkey,
    pub customer: Pubkey,
    pub redeemed_at: i64,
    pub bump: u8,
}

/// Campaign flavors (gamification). Stored as `Campaign.kind`.
pub mod campaign_kind {
    /// Percentage earn boost (`multiplier_bps`) while the window is open.
    pub const MULTIPLIER: u8 = 0;
    /// Fixed `flat_bonus` raw points added to each qualifying earn.
    pub const FLAT_BONUS: u8 = 1;
    /// Visit-goal quest: complete `quest_target` qualifying earns → one-time
    /// `quest_reward` raw-point payout.
    pub const QUEST: u8 = 2;
}

/// A merchant engagement campaign (phase 3, enterprise/gamified). `merchant`
/// first for memcmp catalogs.
#[account]
#[derive(InitSpace)]
pub struct Campaign {
    pub merchant: Pubkey,
    pub id: u64,
    /// `campaign_kind::*`.
    pub kind: u8,
    /// MULTIPLIER: additive earn boost, bps (stacks with streak, jointly capped).
    pub multiplier_bps: u16,
    /// FLAT_BONUS: raw points added per qualifying earn.
    pub flat_bonus: u64,
    /// QUEST: qualifying earns required to complete.
    pub quest_target: u16,
    /// QUEST: raw-point reward on completion.
    pub quest_reward: u64,
    /// Minimum qualifying spend (base minor units) for the campaign to apply.
    pub min_spend_base: u64,
    /// Minimum customer tier (VIP targeting).
    pub min_tier: u8,
    /// Total bonus-point budget; 0 = unlimited. Campaign stops paying when hit.
    pub points_budget: u64,
    pub points_spent: u64,
    /// Max bonus a single customer may draw; 0 = unlimited.
    pub per_customer_cap: u64,
    pub starts_at: i64,
    pub ends_at: i64,
    pub participant_count: u32,
    /// Count of qualifying applications (payouts).
    pub redemptions: u64,
    #[max_len(48)]
    pub name: String,
    pub active: bool,
    pub paused: bool,
    /// Slot at creation. Because a campaign PDA (`[campaign, merchant, id]`) is
    /// reusable after close, this distinguishes one instance of an id from the
    /// next so stale CampaignProgress cannot bleed across (AUDIT M-3).
    pub created_slot: u64,
    pub bump: u8,
}

impl Campaign {
    /// Live = active, not paused, within window, budget not exhausted.
    pub fn is_live(&self, now: i64) -> bool {
        self.active
            && !self.paused
            && self.starts_at <= now
            && now < self.ends_at
            && (self.points_budget == 0 || self.points_spent < self.points_budget)
    }
}

/// Per-(campaign, customer) participation state — enforces per-customer caps
/// and tracks quest progress. Created on first qualifying earn.
#[account]
#[derive(InitSpace)]
pub struct CampaignProgress {
    pub campaign: Pubkey,
    pub customer: Pubkey,
    /// `Campaign.created_slot` of the instance this progress belongs to. A
    /// mismatch means the id was closed and recreated → treat as fresh.
    pub campaign_slot: u64,
    /// Qualifying earns applied (quest counter).
    pub visits: u16,
    /// Bonus raw points drawn by this customer under the campaign.
    pub bonus_drawn: u64,
    /// Quest completed (reward already paid).
    pub completed: bool,
    pub bump: u8,
}

/// Kleos badge definition (phase 3). `merchant` first for memcmp catalogs.
#[account]
#[derive(InitSpace)]
pub struct Achievement {
    pub merchant: Pubkey,
    pub id: u64,
    #[max_len(32)]
    pub name: String,
    #[max_len(128)]
    pub uri: String,
    pub threshold_lifetime: u64,
    pub badge_count: u32,
    pub bump: u8,
}

/// Double-grant guard that survives a holder-side badge burn.
#[account]
#[derive(InitSpace)]
pub struct KleosReceipt {
    pub granted_at: i64,
    pub bump: u8,
}

/// Koinon alliance root; creator in the seeds kills permissionless id squatting.
///
/// Enterprise surface: alliance-level pause, member swap-rate governance bounds,
/// a swap spread (`fee_bps`, an anti-abuse haircut on cross-swaps), a display
/// category + metadata URI, and aggregate swap stats.
#[account]
#[derive(InitSpace)]
pub struct Alliance {
    pub id: u64,
    pub authority: Pubkey,
    pub pending_authority: Option<Pubkey>,
    #[max_len(32)]
    pub name: String,
    pub member_count: u16,
    // ── enterprise fields ──────────────────────────────────────────────────
    /// Freeze all cross-swaps in the alliance.
    pub paused: bool,
    /// Spread applied to each swap's output UI value (anti-abuse / spread), bps.
    pub fee_bps: u16,
    /// Governance bounds on member `rate_bps_to_alliance` (0 = unbounded).
    pub min_rate_bps: u32,
    pub max_rate_bps: u32,
    pub category: u8,
    #[max_len(128)]
    pub metadata_uri: String,
    pub total_swaps: u64,
    pub total_ui_volume: u128,
    pub bump: u8,
}

/// Alliance membership: normalized swap rate + inbound-swap risk budget + stats.
#[account]
#[derive(InitSpace)]
pub struct AllianceMember {
    pub alliance: Pubkey,
    pub merchant: Pubkey,
    pub rate_bps_to_alliance: u32,
    pub swap_in_budget_raw: u64,
    pub swapped_in_today: u64,
    pub budget_day: u32,
    pub active: bool,
    pub joined_at: i64,
    pub total_swapped_in: u64,
    pub total_swapped_out: u64,
    pub bump: u8,
}
