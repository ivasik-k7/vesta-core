use anchor_lang::prelude::*;

use crate::{
    constants::ATTESTATION_SEED,
    error::AegisError,
    events::{AttestationIssued, AttestationRevoked, AttestationUpdated},
    state::{Attestation, Issuer},
};

/// Payload shared by issue/update — one struct, one validation path.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct AttestationData {
    pub schema: u16,
    pub value: u64,
    /// Unix expiry; 0 = never.
    pub expires_at: i64,
}

fn validate(data: &AttestationData) -> Result<()> {
    if data.expires_at != 0 {
        require!(
            data.expires_at > Clock::get()?.unix_timestamp,
            AegisError::InvalidExpiry
        );
    }
    Ok(())
}

#[derive(Accounts)]
#[instruction(subject: Pubkey)]
pub struct IssueAttestation<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(mut, has_one = authority @ AegisError::Unauthorized)]
    pub issuer: Account<'info, Issuer>,

    #[account(
        init,
        payer = authority,
        space = 8 + Attestation::INIT_SPACE,
        seeds = [ATTESTATION_SEED, issuer.key().as_ref(), subject.as_ref()],
        bump,
    )]
    pub attestation: Account<'info, Attestation>,

    pub system_program: Program<'info, System>,
}

pub fn handle_issue_attestation(
    ctx: Context<IssueAttestation>,
    subject: Pubkey,
    data: AttestationData,
) -> Result<()> {
    require!(!ctx.accounts.issuer.paused, AegisError::IssuerPaused);
    validate(&data)?;

    let now = Clock::get()?.unix_timestamp;
    let att = &mut ctx.accounts.attestation;
    att.issuer = ctx.accounts.issuer.key();
    att.subject = subject;
    att.schema = data.schema;
    att.value = data.value;
    att.issued_at = now;
    att.expires_at = data.expires_at;
    att.revoked = false;
    att.bump = ctx.bumps.attestation;

    let issuer = &mut ctx.accounts.issuer;
    issuer.issued = issuer.issued.saturating_add(1);

    emit!(AttestationIssued {
        issuer: att.issuer,
        subject,
        schema: data.schema,
        value: data.value,
        expires_at: data.expires_at,
    });
    Ok(())
}

/// Retune an existing attestation (new value/expiry, or un-revoke by reissue).
#[derive(Accounts)]
pub struct UpdateAttestation<'info> {
    pub authority: Signer<'info>,

    #[account(has_one = authority @ AegisError::Unauthorized)]
    pub issuer: Account<'info, Issuer>,

    #[account(
        mut,
        has_one = issuer @ AegisError::Unauthorized,
        seeds = [ATTESTATION_SEED, issuer.key().as_ref(), attestation.subject.as_ref()],
        bump = attestation.bump,
    )]
    pub attestation: Account<'info, Attestation>,
}

pub fn handle_update_attestation(
    ctx: Context<UpdateAttestation>,
    data: AttestationData,
) -> Result<()> {
    require!(!ctx.accounts.issuer.paused, AegisError::IssuerPaused);
    validate(&data)?;

    let att = &mut ctx.accounts.attestation;
    att.schema = data.schema;
    att.value = data.value;
    att.expires_at = data.expires_at;
    att.revoked = false;

    emit!(AttestationUpdated {
        issuer: att.issuer,
        subject: att.subject,
        schema: data.schema,
        value: data.value,
        expires_at: data.expires_at,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct RevokeAttestation<'info> {
    pub authority: Signer<'info>,

    #[account(has_one = authority @ AegisError::Unauthorized)]
    pub issuer: Account<'info, Issuer>,

    #[account(
        mut,
        has_one = issuer @ AegisError::Unauthorized,
        seeds = [ATTESTATION_SEED, issuer.key().as_ref(), attestation.subject.as_ref()],
        bump = attestation.bump,
    )]
    pub attestation: Account<'info, Attestation>,
}

pub fn handle_revoke_attestation(ctx: Context<RevokeAttestation>) -> Result<()> {
    let att = &mut ctx.accounts.attestation;
    require!(!att.revoked, AegisError::AlreadyRevoked);
    att.revoked = true;
    emit!(AttestationRevoked {
        issuer: att.issuer,
        subject: att.subject,
    });
    Ok(())
}
