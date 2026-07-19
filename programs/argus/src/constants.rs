use anchor_lang::prelude::*;

#[constant]
pub const LEDGER_SEED: &[u8] = b"ledger";

/// Interface-mandated seed for the ExtraAccountMetaList PDA.
#[constant]
pub const EXTRA_ACCOUNT_METAS_SEED: &[u8] = b"extra-account-metas";

/// Daily gift velocity cap per (mint, source-owner), raw units (= 500.00 pts at issue).
#[constant]
pub const DAILY_GIFT_CAP_RAW: u64 = 50_000;

pub const SECONDS_PER_DAY: i64 = 86_400;

/// vesta_core program id — argus deliberately does NOT link the vesta-core
/// crate (a crate dependency would poison the workspace build via
/// no-entrypoint feature unification); cross-checked by an integration test.
pub const VESTA_CORE_ID: Pubkey = pubkey!("Am2X4B1SCnJKXL8Yir2j6yGpHAKrmwcf2E5aKnA9BZV");

/// Anchor discriminator of vesta_core's Merchant account
/// (sha256("account:Merchant")[..8]); equality is asserted in tests.
pub const MERCHANT_DISCRIMINATOR: [u8; 8] = [71, 235, 30, 40, 231, 21, 32, 64];
