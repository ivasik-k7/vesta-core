use anchor_lang::prelude::*;

#[event]
pub struct ConfigInitialized {
    pub admin: Pubkey,
}

#[event]
pub struct ConfigMigrated {
    pub admin: Pubkey,
}

#[event]
pub struct AdminProposed {
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct AdminChanged {
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct PausedSet {
    pub paused: bool,
}

#[event]
pub struct MerchantRegistered {
    pub merchant: Pubkey,
    pub mint: Pubkey,
    pub name: String,
    pub decay_rate_bps: i16,
}

#[event]
pub struct MerchantUpdated {
    pub merchant: Pubkey,
    pub base_earn_rate: u64,
}

#[event]
pub struct PointsEarned {
    pub merchant: Pubkey,
    pub customer: Pubkey,
    pub base: u64,
    pub minted: u64,
    pub multiplier_bps: u64,
    pub streak_days: u16,
}

#[event]
pub struct OfferCreated {
    pub merchant: Pubkey,
    pub offer_id: u64,
    pub price_points: u64,
    pub supply: u32,
}

#[event]
pub struct OfferClosed {
    pub merchant: Pubkey,
    pub offer_id: u64,
}

#[event]
pub struct OfferRedeemed {
    pub offer: Pubkey,
    pub customer: Pubkey,
    pub raw_burned: u64,
    pub receipt: Pubkey,
}

#[event]
pub struct ReceiptClosed {
    pub receipt: Pubkey,
    pub customer: Pubkey,
}

#[event]
pub struct TransferGuardFinalized {
    pub mint: Pubkey,
    pub merchant: Pubkey,
}

#[event]
pub struct CampaignCreated {
    pub merchant: Pubkey,
    pub id: u64,
    pub multiplier_bps: u16,
    pub starts_at: i64,
    pub ends_at: i64,
}

#[event]
pub struct CampaignClosed {
    pub merchant: Pubkey,
    pub id: u64,
}

#[event]
pub struct AchievementCreated {
    pub merchant: Pubkey,
    pub id: u64,
    pub threshold: u64,
}

#[event]
pub struct AchievementGranted {
    pub achievement: Pubkey,
    pub customer: Pubkey,
    pub badge_mint: Pubkey,
}

#[event]
pub struct AllianceCreated {
    pub alliance: Pubkey,
    pub id: u64,
    pub authority: Pubkey,
}

#[event]
pub struct AllianceJoined {
    pub alliance: Pubkey,
    pub merchant: Pubkey,
    pub rate_bps: u32,
    pub swap_in_budget: u64,
}

#[event]
pub struct AllianceLeft {
    pub alliance: Pubkey,
    pub merchant: Pubkey,
}

#[event]
pub struct AllianceAuthorityProposed {
    pub alliance: Pubkey,
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct AllianceAuthorityChanged {
    pub alliance: Pubkey,
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct SwapRateSet {
    pub member: Pubkey,
    pub old: u32,
    pub new: u32,
}

#[event]
pub struct SwapBudgetSet {
    pub member: Pubkey,
    pub old: u64,
    pub new: u64,
}

#[event]
pub struct PointsSwapped {
    pub customer: Pubkey,
    pub merchant_a: Pubkey,
    pub merchant_b: Pubkey,
    pub ui_amount: u64,
    pub raw_in: u64,
    pub raw_out: u64,
}

#[event]
pub struct Clawback {
    pub merchant: Pubkey,
    pub customer: Pubkey,
    pub amount_raw: u64,
    pub reason_code: u16,
}
