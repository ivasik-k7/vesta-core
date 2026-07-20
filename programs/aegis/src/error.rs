use anchor_lang::prelude::*;

#[error_code]
pub enum AegisError {
    #[msg("Issuer name is empty or exceeds the maximum length")]
    InvalidName,
    #[msg("Not authorized for this issuer (authority or operator required)")]
    Unauthorized,
    #[msg("Only the issuer authority may perform this action")]
    AuthorityOnly,
    #[msg("Pending authority does not match the signer")]
    PendingAuthorityMismatch,
    #[msg("Issuer is paused — issuance disabled")]
    IssuerPaused,
    #[msg("Expiry must be zero (never) or after both now and valid_from")]
    InvalidExpiry,
    #[msg("valid_from must be zero or a sane (non-negative) timestamp")]
    InvalidValidFrom,
    #[msg("Attestation already revoked")]
    AlreadyRevoked,
}
