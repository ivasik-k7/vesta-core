use anchor_lang::prelude::*;

#[constant]
pub const CONFIG_SEED: &[u8] = b"config";
#[constant]
pub const MERCHANT_SEED: &[u8] = b"merchant";
#[constant]
pub const MINT_SEED: &[u8] = b"mint";
#[constant]
pub const CUSTOMER_SEED: &[u8] = b"customer";
#[constant]
pub const OFFER_SEED: &[u8] = b"offer";
#[constant]
pub const RECEIPT_SEED: &[u8] = b"receipt";
#[constant]
pub const CAMPAIGN_SEED: &[u8] = b"campaign";
#[constant]
pub const CAMPAIGN_PROGRESS_SEED: &[u8] = b"cprogress";
#[constant]
pub const ACHIEVE_SEED: &[u8] = b"achieve";
#[constant]
pub const KLEOS_SEED: &[u8] = b"kleos";
#[constant]
pub const BADGE_SEED: &[u8] = b"badge";
#[constant]
pub const ALLIANCE_SEED: &[u8] = b"alliance";
#[constant]
pub const MEMBER_SEED: &[u8] = b"member";

/// UI points carry two implied decimals; all mints.
#[constant]
pub const DECIMALS: u8 = 2;

/// Earn cap per transaction, raw units (= 10 000.00 pts).
#[constant]
pub const MAX_EARN_PER_TX: u64 = 1_000_000;

/// +2%/day of streak, capped at 30 days (≤ +6 000 bps).
#[constant]
pub const STREAK_BPS_PER_DAY: u16 = 200;
#[constant]
pub const STREAK_DAYS_CAP: u16 = 30;

/// Per-campaign multiplier bound (2.0×).
#[constant]
pub const CAMPAIGN_MAX_BPS: u16 = 20_000;

/// Bonus-point ceiling for a single flat-bonus / quest payout, raw units
/// (= 10 000.00 pts). Bounds abuse of campaign bonuses.
#[constant]
pub const MAX_CAMPAIGN_BONUS: u64 = 1_000_000;

/// Max quest length (qualifying visits).
#[constant]
pub const MAX_QUEST_TARGET: u16 = 1_000;

/// Alliance swap spread ceiling (10%).
#[constant]
pub const MAX_ALLIANCE_FEE_BPS: u16 = 1_000;

/// Merchant/alliance profile URI length bound.
pub const MAX_METADATA_URI_LEN: usize = 128;

/// Campaign display-name length bound.
pub const MAX_CAMPAIGN_NAME_LEN: usize = 48;

/// Joint cap over streak + campaign composition (2.4×).
#[constant]
pub const MAX_TOTAL_MULTIPLIER_BPS: u64 = 24_000;

/// Tier thresholds on raw lifetime_earned (raw-at-issue: decay never demotes).
pub const TIER_THRESHOLDS: [u64; 4] = [0, 100_000, 1_000_000, 10_000_000];

/// `base_earn_rate` bounds: raw points per fiat minor unit (cent).
#[constant]
pub const MIN_BASE_EARN_RATE: u64 = 1;
#[constant]
pub const MAX_BASE_EARN_RATE: u64 = 1_000;

pub const MAX_NAME_LEN: usize = 32;
pub const MAX_SYMBOL_LEN: usize = 10;
pub const MAX_URI_LEN: usize = 200;

pub const SECONDS_PER_DAY: i64 = 86_400;
