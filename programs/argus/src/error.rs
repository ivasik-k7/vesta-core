use anchor_lang::prelude::*;

#[error_code]
pub enum GuardError {
    #[msg("Guard initialization not authorized for this merchant/mint")]
    UnauthorizedGuardInit,
    #[msg("Transfer guard already initialized for this mint")]
    GuardAlreadyInitialized,
    #[msg("Mint does not match the merchant point mint")]
    MintMismatch,
    #[msg("Gift ledger not opened — run open_gift_ledger first")]
    LedgerNotOpened,
    #[msg("Daily gift cap exceeded")]
    GiftCapExceeded,
    #[msg("Destination wallet is program-owned — not a loyalty flow")]
    ProgramOwnedDestination,
    #[msg("Provided extra account does not match the meta list derivation")]
    MetaListMismatch,
    #[msg("Transfer flow not allowed")]
    FlowNotAllowed,
    #[msg("Arithmetic overflow")]
    Overflow,
}
