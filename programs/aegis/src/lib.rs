//! Aegis — the shield of VESTA.
//!
//! A privacy-preserving attestation & trust layer (docs/specs/06, 07): issuers
//! publish only hiding commitments (PII off-chain), and any program consumes
//! them through the stable `verify` / `verify_policy` interface — a verdict via
//! return-data — rather than reading account layouts. argus gates transfers on
//! these verdicts; vesta_core campaigns can gate issuance the same way.

pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("AcCdMQC1rj4KukjhFzf4S8metEAXpnt9gzvMThsu15e1");

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "VESTA Aegis — attestation issuer",
    project_url: "https://github.com/ivasik-k7/vesta-core",
    contacts: "email:kovtun.ivan@proton.me,link:https://github.com/ivasik-k7/vesta-core/blob/main/SECURITY.md",
    policy: "https://github.com/ivasik-k7/vesta-core/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/ivasik-k7/vesta-core",
    source_revision: "main",
    auditors: "None"
}

#[program]
pub mod aegis {
    use super::*;

    /// Create an attestation authority (one per creator wallet).
    pub fn init_issuer(ctx: Context<InitIssuer>, id: u64, name: String) -> Result<()> {
        instructions::issuer::handle_init_issuer(ctx, id, name)
    }

    /// Pause / resume issuance for this issuer (authority only).
    pub fn set_issuer_paused(ctx: Context<IssuerAuthorityOnly>, paused: bool) -> Result<()> {
        instructions::issuer::handle_set_issuer_paused(ctx, paused)
    }

    /// Set or clear the hot operator key (authority only).
    pub fn set_operator(ctx: Context<IssuerAuthorityOnly>, operator: Option<Pubkey>) -> Result<()> {
        instructions::issuer::handle_set_operator(ctx, operator)
    }

    /// Propose a new issuer authority (two-step, authority only).
    pub fn transfer_issuer_authority(
        ctx: Context<IssuerAuthorityOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        instructions::issuer::handle_transfer_issuer_authority(ctx, new_authority)
    }

    /// Accept a proposed issuer authority (two-step).
    pub fn accept_issuer_authority(ctx: Context<AcceptIssuerAuthority>) -> Result<()> {
        instructions::issuer::handle_accept_issuer_authority(ctx)
    }

    /// Register a typed, versioned credential schema (a shape, no subject data).
    pub fn register_schema(
        ctx: Context<RegisterSchema>,
        id: u64,
        content_hash: [u8; 32],
        standard_uri: String,
        sas_schema: Option<Pubkey>,
    ) -> Result<()> {
        instructions::schema::handle_register_schema(
            ctx,
            id,
            content_hash,
            standard_uri,
            sas_schema,
        )
    }

    /// Deprecate a schema, optionally pointing at a successor (registrar only).
    pub fn deprecate_schema(
        ctx: Context<DeprecateSchema>,
        successor: Option<Pubkey>,
    ) -> Result<()> {
        instructions::schema::handle_deprecate_schema(ctx, successor)
    }

    /// Issue a fresh attestation for a subject (authority or operator). The
    /// chain stores only a commitment + Merkle root — never plaintext claims.
    pub fn issue_attestation(
        ctx: Context<IssueAttestation>,
        subject: Pubkey,
        data: AttestationData,
    ) -> Result<()> {
        instructions::attestation::handle_issue_attestation(ctx, subject, data)
    }

    /// Retune an existing attestation (authority or operator). Revocation is
    /// terminal — a revoked attestation is rejected, never reinstated here.
    pub fn update_attestation(
        ctx: Context<ManageAttestation>,
        data: AttestationData,
    ) -> Result<()> {
        instructions::attestation::handle_update_attestation(ctx, data)
    }

    /// Revoke an attestation — downstream guards fail closed immediately.
    pub fn revoke_attestation(ctx: Context<ManageAttestation>, reason_code: u16) -> Result<()> {
        instructions::attestation::handle_revoke_attestation(ctx, reason_code)
    }

    /// Cryptographically erase an attestation (GDPR): off-chain PII + salt
    /// destroyed, the commitment is now unopenable. Terminal.
    pub fn erase_attestation(ctx: Context<ManageAttestation>) -> Result<()> {
        instructions::attestation::handle_erase_attestation(ctx)
    }

    /// Close an attestation and reclaim rent to the issuer authority.
    pub fn close_attestation(ctx: Context<CloseAttestation>) -> Result<()> {
        instructions::attestation::handle_close_attestation(ctx)
    }

    /// Stateless verdict primitive (spec 07): evaluate a predicate over an
    /// attestation and return a `Verdict` via return-data. Any program CPIs
    /// this instead of reading aegis accounts by layout.
    pub fn verify(ctx: Context<Verify>, predicate: VerifyPredicate) -> Result<()> {
        instructions::verify::handle_verify(ctx, predicate)
    }

    /// Register a named, versioned, jurisdiction-tagged verifier policy (spec 07).
    pub fn register_policy(
        ctx: Context<RegisterPolicy>,
        id: u64,
        jurisdiction: u16,
        issuer: Pubkey,
        schema_id: u64,
        freshness_secs: i64,
    ) -> Result<()> {
        instructions::policy::handle_register_policy(
            ctx,
            id,
            jurisdiction,
            issuer,
            schema_id,
            freshness_secs,
        )
    }

    /// Deprecate a policy, optionally naming a successor (authority only).
    pub fn deprecate_policy(
        ctx: Context<DeprecatePolicy>,
        successor: Option<Pubkey>,
    ) -> Result<()> {
        instructions::policy::handle_deprecate_policy(ctx, successor)
    }

    /// Evaluate a subject against a named policy — reproducible, jurisdiction-
    /// aware, freshness-checked. Returns a `Verdict` via return-data and emits a
    /// `PolicyDecision` audit event stamped with the policy version.
    pub fn verify_policy(ctx: Context<VerifyPolicy>, subject: Pubkey) -> Result<()> {
        instructions::policy::handle_verify_policy(ctx, subject)
    }

    /// Declare a trust root (spec 08) — an entity verifiers may pin so its
    /// accredited issuers inherit trust. Permissionless to declare.
    pub fn register_trust_root(ctx: Context<RegisterTrustRoot>, name: String) -> Result<()> {
        instructions::accreditation::handle_register_trust_root(ctx, name)
    }

    /// Accredit an issuer under the signer's trust root (spec 08).
    pub fn accredit_issuer(
        ctx: Context<AccreditIssuer>,
        subject_issuer: Pubkey,
        tier: u8,
        permitted_schemas: Vec<u64>,
        jurisdiction: u16,
        expires_at: i64,
    ) -> Result<()> {
        instructions::accreditation::handle_accredit_issuer(
            ctx,
            subject_issuer,
            tier,
            permitted_schemas,
            jurisdiction,
            expires_at,
        )
    }

    /// Revoke an accreditation — de-trusts the issuer under this root instantly.
    pub fn revoke_accreditation(ctx: Context<RevokeAccreditation>) -> Result<()> {
        instructions::accreditation::handle_revoke_accreditation(ctx)
    }

    /// Stateless verdict (spec 08): is `subject_issuer` accredited by `root` for
    /// `schema_id`? Returned via return-data — compose it with a credential
    /// `verify` so a verifier trusts a root instead of a hardcoded issuer key.
    pub fn verify_accreditation(
        ctx: Context<VerifyAccreditation>,
        root: Pubkey,
        subject_issuer: Pubkey,
        schema_id: u64,
    ) -> Result<()> {
        instructions::accreditation::handle_verify_accreditation(
            ctx,
            root,
            subject_issuer,
            schema_id,
        )
    }
}
