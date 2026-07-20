use anchor_lang::prelude::*;

#[error_code]
pub enum AegisError {
    #[msg("Issuer name exceeds the maximum length")]
    NameTooLong,
    #[msg("Not the issuer authority")]
    Unauthorized,
    #[msg("Pending authority does not match the signer")]
    PendingAuthorityMismatch,
    #[msg("Issuer is paused — issuance disabled")]
    IssuerPaused,
    #[msg("Expiry must be in the future or zero (never)")]
    InvalidExpiry,
    #[msg("Attestation already revoked")]
    AlreadyRevoked,
}
