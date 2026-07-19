use anchor_lang::prelude::*;

#[event]
pub struct TransferGuardInitialized {
    pub mint: Pubkey,
    pub merchant: Pubkey,
}

#[event]
pub struct GiftLedgerOpened {
    pub mint: Pubkey,
    pub owner: Pubkey,
}

#[event]
pub struct PointsGifted {
    pub mint: Pubkey,
    pub source_owner: Pubkey,
    pub destination: Pubkey,
    pub amount: u64,
    pub gifted_today: u64,
}

#[event]
pub struct ClawbackObserved {
    pub mint: Pubkey,
    pub source_owner: Pubkey,
    pub amount: u64,
}
