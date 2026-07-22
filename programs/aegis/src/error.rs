use anchor_lang::prelude::*;

#[error_code]
pub enum AegisError {
    #[msg("Issuer name is empty or exceeds the maximum length")]
    InvalidName,
    #[msg("String exceeds the maximum length")]
    StringTooLong,
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
    #[msg("Attestation is revoked or erased — terminal, cannot be modified")]
    AlreadyRevoked,
    #[msg("Schema is deprecated")]
    SchemaDeprecated,
    #[msg("Schema id mismatch between the attestation and the schema account")]
    SchemaMismatch,
    #[msg("Merkle disclosure path exceeds the maximum depth")]
    DisclosureTooDeep,
    #[msg("Account layout version is not supported")]
    UnsupportedVersion,
    #[msg("Unknown or malformed predicate")]
    UnknownPredicate,
}
