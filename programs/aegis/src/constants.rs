use anchor_lang::prelude::*;

/// Issuer authority account. Seeds ["issuer", authority, id_le].
#[constant]
pub const ISSUER_SEED: &[u8] = b"issuer";

/// Per-(issuer, subject, schema) credential. Consumers read it through the
/// `verify` interface (never by fixed byte offset — spec 07); every account
/// carries a `version` header so storage can evolve without breaking readers.
/// Seeds ["attestation", issuer, subject, schema_id_le].
#[constant]
pub const ATTESTATION_SEED: &[u8] = b"attestation";

/// Typed credential schema. Seeds ["schema", registrar, schema_id_le].
#[constant]
pub const SCHEMA_SEED: &[u8] = b"schema";

/// Named verifier policy (spec 07). Seeds ["policy", authority, policy_id_le].
#[constant]
pub const POLICY_SEED: &[u8] = b"policy";

/// Trust root (spec 08). Seeds ["troot", authority].
#[constant]
pub const TRUST_ROOT_SEED: &[u8] = b"troot";

/// Issuer accreditation edge (spec 08). Seeds ["accred", root, subject_issuer].
#[constant]
pub const ACCREDITATION_SEED: &[u8] = b"accred";

/// Max schemas one accreditation may permit (0 used = all schemas).
pub const MAX_PERMITTED_SCHEMAS: usize = 8;

/// Account layout version (Track B convention).
pub const STATE_VERSION: u8 = 1;

/// Max issuer display-name length, bytes.
pub const MAX_NAME_LEN: usize = 48;

/// Max schema `standard_uri` length, bytes (W3C VC type / mdoc namespace).
pub const MAX_STANDARD_URI_LEN: usize = 128;

/// Max Merkle depth for `attr_root` disclosure proofs (bounds verify CU).
pub const MAX_ATTR_DEPTH: usize = 8;

/// Well-known schema ids (advisory — issuers may define their own).
pub mod well_known_schema {
    /// Region / geofencing credential.
    pub const REGION: u64 = 1;
    /// KYC tier credential.
    pub const KYC_TIER: u64 = 2;
    /// Age-band credential.
    pub const AGE_BAND: u64 = 3;
}

/// `verify` verdict reason codes (returned in `Verdict.reason_code`).
pub mod verify_reason {
    pub const OK: u16 = 0;
    pub const NOT_FOUND: u16 = 1;
    pub const WRONG_ISSUER: u16 = 2;
    pub const WRONG_SCHEMA: u16 = 3;
    pub const NOT_ACTIVE: u16 = 4;
    pub const OUT_OF_WINDOW: u16 = 5;
    pub const DISCLOSURE_MISMATCH: u16 = 6;
    pub const THRESHOLD_UNMET: u16 = 7;
    pub const UNKNOWN_PREDICATE: u16 = 8;
    /// Credential older than the policy's freshness / re-verification window.
    pub const TOO_OLD: u16 = 9;
    /// Trust root missing or inactive (spec 08).
    pub const ROOT_INACTIVE: u16 = 10;
    /// Issuer is not accredited by the trust root (spec 08).
    pub const NOT_ACCREDITED: u16 = 11;
    /// Accreditation does not permit the requested schema (spec 08).
    pub const SCHEMA_NOT_PERMITTED: u16 = 12;
    /// The named policy has been deprecated / retired (spec 07).
    pub const POLICY_DEPRECATED: u16 = 13;
}
