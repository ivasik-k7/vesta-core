use anchor_lang::prelude::*;

#[error_code]
pub enum VestaError {
    #[msg("Protocol is paused")]
    ProtocolPaused,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Pending admin does not match signer")]
    PendingAdminMismatch,
    #[msg("Config migration already applied")]
    MigrationAlreadyApplied,
    #[msg("Decay rate must be within -10000..=0 bps")]
    InvalidDecayRate,
    #[msg("Base earn rate out of bounds")]
    InvalidEarnRate,
    #[msg("Only 2-decimal point mints are supported")]
    InvalidDecimals,
    #[msg("String exceeds maximum length")]
    StringTooLong,
    #[msg("visit_day does not match the current UTC day")]
    StaleVisitDay,
    #[msg("Earn amount exceeds the per-transaction cap")]
    EarnCapExceeded,
    #[msg("Multiplier arithmetic overflow")]
    MultiplierOverflow,
    #[msg("Campaign is not active in the current window")]
    CampaignInactive,
    #[msg("Campaign window is invalid")]
    CampaignWindowInvalid,
    #[msg("Offer is not active")]
    OfferInactive,
    #[msg("Offer supply is exhausted")]
    OfferSoldOut,
    #[msg("Slippage bound exceeded")]
    SlippageExceeded,
    #[msg("Mint does not match the merchant point mint")]
    MintMismatch,
    #[msg("Merchant account mismatch")]
    MerchantMismatch,
    #[msg("Treasury account mismatch")]
    TreasuryMismatch,
    #[msg("Alliance mismatch")]
    AllianceMismatch,
    #[msg("Alliance member is not active")]
    MemberInactive,
    #[msg("Merchant already belongs to an alliance")]
    AlreadyInAlliance,
    #[msg("Daily swap-in budget exceeded")]
    SwapBudgetExceeded,
    #[msg("Invalid swap rate")]
    InvalidSwapRate,
    #[msg("Transfer guard is not initialized")]
    GuardNotInitialized,
    #[msg("Transfer guard already finalized")]
    GuardAlreadyFinalized,
    #[msg("Achievement threshold not reached")]
    ThresholdNotReached,
    #[msg("Achievement already granted")]
    AlreadyGranted,
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("Conversion return data missing or malformed")]
    ConversionFailed,
}
