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
pub struct MerchantClosed {
    pub merchant: Pubkey,
    pub id: u64,
    pub authority: Pubkey,
}

#[event]
pub struct TokenAttributeSet {
    pub merchant: Pubkey,
    pub mint: Pubkey,
    pub key: String,
    pub value: String,
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
pub struct OfferSegmentSet {
    pub merchant: Pubkey,
    pub offer_id: u64,
    pub required_segment: u8,
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
    pub kind: u8,
    pub multiplier_bps: u16,
    pub starts_at: i64,
    pub ends_at: i64,
}

#[event]
pub struct CampaignUpdated {
    pub merchant: Pubkey,
    pub id: u64,
    pub paused: bool,
    pub points_budget: u64,
    pub ends_at: i64,
}

#[event]
pub struct CampaignBonusPaid {
    pub merchant: Pubkey,
    pub campaign: u64,
    pub customer: Pubkey,
    pub kind: u8,
    pub bonus: u64,
    pub quest_completed: bool,
}

#[event]
pub struct CampaignWinbackSet {
    pub merchant: Pubkey,
    pub id: u64,
    pub min_days_inactive: u16,
}

#[event]
pub struct CampaignClosed {
    pub merchant: Pubkey,
    pub id: u64,
}

#[event]
pub struct MerchantOperatorSet {
    pub merchant: Pubkey,
    pub operator: Pubkey,
    pub added: bool,
}

#[event]
pub struct MerchantPausedSet {
    pub merchant: Pubkey,
    pub paused: bool,
}

#[event]
pub struct MerchantVerifiedSet {
    pub merchant: Pubkey,
    pub verified: bool,
}

#[event]
pub struct MerchantProfileUpdated {
    pub merchant: Pubkey,
    pub category: u8,
}

// ── Accreditation (spec 11) ──────────────────────────────────────────────────

#[event]
pub struct MerchantTrustSet {
    pub merchant: Pubkey,
    pub accreditation_root: Pubkey,
    pub subject_issuer: Pubkey,
    pub required_schema: u64,
    pub degrade_target: u8,
}

#[event]
pub struct MerchantReverified {
    pub merchant: Pubkey,
    pub healthy: bool,
    pub issue_status: u8,
    pub reason_code: u16,
}

#[event]
pub struct MerchantIssueStatusSet {
    pub merchant: Pubkey,
    pub old: u8,
    pub new: u8,
    /// True when set by the permissionless crank; false for a manual override.
    pub automatic: bool,
}

#[event]
pub struct IssuanceCapSet {
    pub merchant: Pubkey,
    pub daily_cap_raw: u64,
}

#[event]
pub struct MerchantGovernanceSet {
    pub merchant: Pubkey,
    pub enabled: bool,
    pub cashier: Pubkey,
    pub campaign_manager: Pubkey,
}

// ── Verified segmentation (spec 12) ──────────────────────────────────────────

#[event]
pub struct MerchantSegmentsSet {
    pub merchant: Pubkey,
    pub policy_epoch: u64,
}

#[event]
pub struct CustomerEligibilityRefreshed {
    pub merchant: Pubkey,
    pub customer: Pubkey,
    pub segment_index: u8,
    pub satisfied: bool,
    pub verdicts: u32,
    pub expires_at: i64,
}

// ── Decision statements (spec 13 §4.4) ───────────────────────────────────────

#[event]
pub struct MerchantStatementAnchored {
    pub merchant: Pubkey,
    pub period: u64,
    pub merkle_root: [u8; 32],
    pub decision_count: u64,
    pub reporter: Pubkey,
}

#[event]
pub struct ReserveOpened {
    pub merchant: Pubkey,
    pub backing_mint: Pubkey,
    pub unit_value: u64,
    pub target_ratio_bps: u16,
}

#[event]
pub struct ReserveFunded {
    pub merchant: Pubkey,
    pub amount: u64,
    pub reserve_balance: u64,
}

#[event]
pub struct ReserveWithdrawn {
    pub merchant: Pubkey,
    pub amount: u64,
    pub reserve_balance: u64,
}

/// Proof-of-reserves snapshot (spec 11 §4.2) — permissionless, examiner-facing.
#[event]
pub struct ReserveAttested {
    pub merchant: Pubkey,
    pub outstanding_raw: u64,
    pub reserve_stable: u64,
    pub required_stable: u64,
    pub solvent: bool,
    pub ts: i64,
}

#[event]
pub struct AlliancePausedSet {
    pub alliance: Pubkey,
    pub paused: bool,
}

#[event]
pub struct AllianceParamsSet {
    pub alliance: Pubkey,
    pub fee_bps: u16,
    pub min_rate_bps: u32,
    pub max_rate_bps: u32,
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
    /// The key that authorized this clawback (owner or operator).
    pub actor: Pubkey,
    pub amount_raw: u64,
    pub reason_code: u16,
    /// Customer's remaining balance after the clawback.
    pub balance_after: u64,
    /// Merchant's cumulative raw amount clawed back today.
    pub clawed_today: u64,
}

#[event]
pub struct ClawbackCapSet {
    pub merchant: Pubkey,
    pub daily_cap_raw: u64,
}

#[event]
pub struct TokenMetadataUpdated {
    pub merchant: Pubkey,
    pub mint: Pubkey,
    pub field_kind: u8,
}

#[event]
pub struct DecayRateUpdated {
    pub merchant: Pubkey,
    pub new_rate_bps: i16,
}

#[event]
pub struct AchievementClosed {
    pub merchant: Pubkey,
    pub id: u64,
}

#[event]
pub struct MemberActiveSet {
    pub alliance: Pubkey,
    pub merchant: Pubkey,
    pub active: bool,
}
