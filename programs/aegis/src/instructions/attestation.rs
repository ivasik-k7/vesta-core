use anchor_lang::prelude::*;

use crate::{
    constants::ATTESTATION_SEED,
    error::AegisError,
    events::{AttestationClosed, AttestationIssued, AttestationRevoked, AttestationUpdated},
    state::{Attestation, Issuer},
};

/// Payload shared by issue/update — one struct, one validation path.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct AttestationData {
    pub schema: u16,
    pub value: u64,
    /// Unix "not-before"; 0 = valid immediately.
    pub valid_from: i64,
    /// Unix expiry; 0 = never.
    pub expires_at: i64,
}

fn validate(data: &AttestationData) -> Result<()> {
    require!(data.valid_from >= 0, AegisError::InvalidValidFrom);
    if data.expires_at != 0 {
        let now = Clock::get()?.unix_timestamp;
        require!(data.expires_at > now, AegisError::InvalidExpiry);
        require!(data.expires_at > data.valid_from, AegisError::InvalidExpiry);
    }
    Ok(())
}

#[derive(Accounts)]
#[instruction(subject: Pubkey)]
pub struct IssueAttestation<'info> {
    /// Authority or the issuer's hot operator.
    #[account(mut)]
    pub signer: Signer<'info>,

    #[account(mut)]
    pub issuer: Account<'info, Issuer>,

    #[account(
        init,
        payer = signer,
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
    require!(
        ctx.accounts
            .issuer
            .is_signer_authorized(&ctx.accounts.signer.key()),
        AegisError::Unauthorized
    );
    require!(!ctx.accounts.issuer.paused, AegisError::IssuerPaused);
    validate(&data)?;

    let now = Clock::get()?.unix_timestamp;
    let att = &mut ctx.accounts.attestation;
    att.issuer = ctx.accounts.issuer.key();
    att.subject = subject;
    att.schema = data.schema;
    att.value = data.value;
    att.issued_at = now;
    att.valid_from = data.valid_from;
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
        valid_from: data.valid_from,
        expires_at: data.expires_at,
    });
    Ok(())
}

/// Context for operating on an existing attestation (update / revoke).
#[derive(Accounts)]
pub struct ManageAttestation<'info> {
    pub signer: Signer<'info>,

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
    ctx: Context<ManageAttestation>,
    data: AttestationData,
) -> Result<()> {
    require!(
        ctx.accounts
            .issuer
            .is_signer_authorized(&ctx.accounts.signer.key()),
        AegisError::Unauthorized
    );
    require!(!ctx.accounts.issuer.paused, AegisError::IssuerPaused);
    // Revocation is terminal — a revoked credential cannot be silently
    // reinstated by an update; the issuer must close and re-issue.
    require!(
        !ctx.accounts.attestation.revoked,
        AegisError::AlreadyRevoked
    );
    validate(&data)?;

    let att = &mut ctx.accounts.attestation;
    att.schema = data.schema;
    att.value = data.value;
    att.valid_from = data.valid_from;
    att.expires_at = data.expires_at;

    emit!(AttestationUpdated {
        issuer: att.issuer,
        subject: att.subject,
        schema: data.schema,
        value: data.value,
        valid_from: data.valid_from,
        expires_at: data.expires_at,
    });
    Ok(())
}

pub fn handle_revoke_attestation(ctx: Context<ManageAttestation>, reason_code: u16) -> Result<()> {
    require!(
        ctx.accounts
            .issuer
            .is_signer_authorized(&ctx.accounts.signer.key()),
        AegisError::Unauthorized
    );
    let att = &mut ctx.accounts.attestation;
    require!(!att.revoked, AegisError::AlreadyRevoked);
    att.revoked = true;
    emit!(AttestationRevoked {
        issuer: att.issuer,
        subject: att.subject,
        reason_code,
    });
    Ok(())
}

/// Close an attestation and reclaim its rent to the issuer authority (the cold
/// key that owns the account's economics), regardless of who signs.
#[derive(Accounts)]
pub struct CloseAttestation<'info> {
    pub signer: Signer<'info>,

    #[account(has_one = authority @ AegisError::Unauthorized)]
    pub issuer: Account<'info, Issuer>,

    /// CHECK: rent recipient, pinned to the issuer authority by has_one above.
    #[account(mut, address = issuer.authority)]
    pub authority: UncheckedAccount<'info>,

    #[account(
        mut,
        close = authority,
        has_one = issuer @ AegisError::Unauthorized,
        seeds = [ATTESTATION_SEED, issuer.key().as_ref(), attestation.subject.as_ref()],
        bump = attestation.bump,
    )]
    pub attestation: Account<'info, Attestation>,
}

pub fn handle_close_attestation(ctx: Context<CloseAttestation>) -> Result<()> {
    require!(
        ctx.accounts
            .issuer
            .is_signer_authorized(&ctx.accounts.signer.key()),
        AegisError::Unauthorized
    );
    emit!(AttestationClosed {
        issuer: ctx.accounts.issuer.key(),
        subject: ctx.accounts.attestation.subject,
    });
    Ok(())
}
