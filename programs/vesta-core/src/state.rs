use anchor_lang::prelude::*;

/// Global protocol config. Singleton PDA.
#[account]
#[derive(InitSpace)]
pub struct Config {
    pub admin: Pubkey,
    pub pending_admin: Option<Pubkey>,
    pub paused: bool,
    pub bump: u8,
}

/// One per merchant; authority is baked into the seeds (no key rotation in-scope).
#[account]
#[derive(InitSpace)]
pub struct Merchant {
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
    pub bump: u8,
    pub mint_bump: u8,
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

/// Earn multiplier campaign (phase 3). `merchant` first for memcmp catalogs.
#[account]
#[derive(InitSpace)]
pub struct Campaign {
    pub merchant: Pubkey,
    pub id: u64,
    pub multiplier_bps: u16,
    pub starts_at: i64,
    pub ends_at: i64,
    pub active: bool,
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
#[account]
#[derive(InitSpace)]
pub struct Alliance {
    pub id: u64,
    pub authority: Pubkey,
    pub pending_authority: Option<Pubkey>,
    #[max_len(32)]
    pub name: String,
    pub member_count: u16,
    pub bump: u8,
}

/// Alliance membership: normalized swap rate + inbound-swap risk budget.
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
    pub bump: u8,
}
