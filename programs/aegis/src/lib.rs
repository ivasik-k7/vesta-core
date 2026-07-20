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

#[program]
pub mod aegis {
    use super::*;

    /// Create an attestation authority (one per creator wallet).
    pub fn init_issuer(ctx: Context<InitIssuer>, name: String) -> Result<()> {
        instructions::issuer::handle_init_issuer(ctx, name)
    }

    /// Pause / resume issuance for this issuer.
    pub fn set_issuer_paused(ctx: Context<IssuerAuthorityOnly>, paused: bool) -> Result<()> {
        instructions::issuer::handle_set_issuer_paused(ctx, paused)
    }

    /// Propose a new issuer authority (two-step).
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

    /// Issue a fresh attestation for a subject wallet.
    pub fn issue_attestation(
        ctx: Context<IssueAttestation>,
        subject: Pubkey,
        data: AttestationData,
    ) -> Result<()> {
        instructions::attestation::handle_issue_attestation(ctx, subject, data)
    }

    /// Retune an existing attestation (also un-revokes).
    pub fn update_attestation(
        ctx: Context<UpdateAttestation>,
        data: AttestationData,
    ) -> Result<()> {
        instructions::attestation::handle_update_attestation(ctx, data)
    }

    /// Revoke an attestation — downstream guards fail closed immediately.
    pub fn revoke_attestation(ctx: Context<RevokeAttestation>) -> Result<()> {
        instructions::attestation::handle_revoke_attestation(ctx)
    }
}
