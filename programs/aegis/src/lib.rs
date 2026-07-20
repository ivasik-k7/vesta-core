//! Aegis — the shield of VESTA.
//!
//! An attestation issuer: authorities sign region / KYC-tier / age credentials
//! into per-subject accounts that downstream programs gate on (see
//! docs/ARGUS_SPEC.md §7, §13). argus reads an `Attestation` by fixed byte
//! offset to enforce geofenced and compliance-gated transfers; vesta_core
//! campaigns can gate issuance the same way. aegis knows nothing about their
//! business logic — the two compose through a documented account layout.

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

    /// Issue a fresh attestation for a subject wallet (authority or operator).
    pub fn issue_attestation(
        ctx: Context<IssueAttestation>,
        subject: Pubkey,
        data: AttestationData,
    ) -> Result<()> {
        instructions::attestation::handle_issue_attestation(ctx, subject, data)
    }

    /// Retune an existing attestation, also clearing revocation (authority or operator).
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

    /// Close an attestation and reclaim rent to the issuer authority.
    pub fn close_attestation(ctx: Context<CloseAttestation>) -> Result<()> {
        instructions::attestation::handle_close_attestation(ctx)
    }
}
