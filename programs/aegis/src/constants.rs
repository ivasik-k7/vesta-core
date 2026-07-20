use anchor_lang::prelude::*;

/// Issuer authority account (spec §13). Seeds ["issuer", authority].
#[constant]
pub const ISSUER_SEED: &[u8] = b"issuer";

/// Per-(issuer, subject) credential. Seeds ["attestation", issuer, subject].
/// The layout is read cross-program by argus at fixed offsets — do not reorder
/// the `Attestation` fields without updating argus::constants::attestation_offset.
#[constant]
pub const ATTESTATION_SEED: &[u8] = b"attestation";

/// Max issuer display-name length, bytes.
pub const MAX_NAME_LEN: usize = 48;

/// Well-known schema ids (advisory — issuers may define their own).
pub mod schema {
    /// `value` is a bitmask of ISO-3166-style region bits (geofencing).
    pub const REGION: u16 = 1;
    /// `value` is a bitmask of KYC tier bits.
    pub const KYC_TIER: u16 = 2;
    /// `value` is a bitmask of age-band bits.
    pub const AGE_BAND: u16 = 3;
}
