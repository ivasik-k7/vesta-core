use anchor_lang::prelude::*;

#[error_code]
pub enum GuardError {
    #[msg("Guard initialization not authorized for this merchant/mint")]
    UnauthorizedGuardInit,
    #[msg("Transfer guard already initialized for this mint")]
    GuardAlreadyInitialized,
    #[msg("Mint does not match the merchant point mint")]
    MintMismatch,
    #[msg("Wallet policy state not opened — run open_wallet_state first")]
    StateNotOpened,
    #[msg("Not the guard authority")]
    Unauthorized,
    #[msg("Pending authority does not match the signer")]
    PendingAuthorityMismatch,
    #[msg("Policy values are inconsistent (e.g. per-tx cap above daily cap)")]
    InvalidPolicy,
    #[msg("Unknown policy flag bit set")]
    UnknownFlag,
    #[msg("Per-mint transfers are paused")]
    MintPaused,
    #[msg("Peer gifting is disabled for this mint")]
    GiftingDisabled,
    #[msg("Per-transfer cap exceeded")]
    PerTxExceeded,
    #[msg("Destination balance cap would be exceeded")]
    BalanceCapExceeded,
    #[msg("Transfer cooldown has not elapsed")]
    CooldownActive,
    #[msg("Daily transfer count cap reached")]
    TransferCountExceeded,
    #[msg("Daily gift cap exceeded")]
    GiftCapExceeded,
    #[msg("Destination is not in the allow list")]
    NotAllowlisted,
    #[msg("Destination is in the deny list")]
    DenyListed,
    #[msg("Attestation missing, expired, revoked, or does not satisfy policy")]
    AttestationFailed,
    #[msg("Destination wallet is program-owned — not a loyalty flow")]
    ProgramOwnedDestination,
    #[msg("Provided extra account does not match the meta list derivation")]
    MetaListMismatch,
    #[msg("execute invoked outside a genuine Token-2022 transfer")]
    NotTransferring,
    #[msg("Eligibility capability missing or stale — run refresh_eligibility")]
    EligibilityStale,
    #[msg("aegis program does not match the guard's configured aegis deployment")]
    AegisProgramMismatch,
    #[msg("Account layout version is not supported")]
    UnsupportedVersion,
    #[msg("Arithmetic overflow")]
    Overflow,
}
