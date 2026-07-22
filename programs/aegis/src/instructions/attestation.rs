use anchor_lang::prelude::*;

use crate::{
    constants::{ATTESTATION_SEED, STATE_VERSION},
    error::AegisError,
    events::{
        AttestationClosed, AttestationErased, AttestationIssued, AttestationRevoked,
        AttestationUpdated,
    },
    state::{attestation_status, Attestation, Issuer},
};

/// Payload shared by issue/update — the chain receives only commitments, never
/// plaintext claims (spec 06). The issuer computes the commitment and the
/// per-attribute Merkle root off-chain from the real credential + salt.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct AttestationData {
    pub schema_id: u64,
    pub commitment: [u8; 32],
    pub attr_root: [u8; 32],
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
#[instruction(subject: Pubkey, data: AttestationData)]
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
        seeds = [ATTESTATION_SEED, issuer.key().as_ref(), subject.as_ref(), &data.schema_id.to_le_bytes()],
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
    att.version = STATE_VERSION;
    att.issuer = ctx.accounts.issuer.key();
    att.subject = subject;
    att.schema_id = data.schema_id;
    att.commitment = data.commitment;
    att.attr_root = data.attr_root;
    att.issued_at = now;
    att.valid_from = data.valid_from;
    att.expires_at = data.expires_at;
    att.status = attestation_status::ACTIVE;
    att.bump = ctx.bumps.attestation;

    let issuer = &mut ctx.accounts.issuer;
    issuer.issued = issuer.issued.saturating_add(1);

    emit!(AttestationIssued {
        issuer: att.issuer,
        subject,
        schema_id: data.schema_id,
        valid_from: data.valid_from,
        expires_at: data.expires_at,
    });
    Ok(())
}

/// Context for operating on an existing attestation (update / revoke / erase).
#[derive(Accounts)]
pub struct ManageAttestation<'info> {
    pub signer: Signer<'info>,

    pub issuer: Account<'info, Issuer>,

    #[account(
        mut,
        has_one = issuer @ AegisError::Unauthorized,
        seeds = [
            ATTESTATION_SEED,
            issuer.key().as_ref(),
            attestation.subject.as_ref(),
            &attestation.schema_id.to_le_bytes(),
        ],
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
    // Revocation / erasure is terminal — a dead credential cannot be silently
    // reinstated by an update; the issuer must close and re-issue.
    require!(
        ctx.accounts.attestation.status == attestation_status::ACTIVE,
        AegisError::AlreadyRevoked
    );
    // The schema of a credential is fixed at issuance (it is a PDA seed).
    require!(
        data.schema_id == ctx.accounts.attestation.schema_id,
        AegisError::SchemaMismatch
    );
    validate(&data)?;

    let att = &mut ctx.accounts.attestation;
    att.commitment = data.commitment;
    att.attr_root = data.attr_root;
    att.valid_from = data.valid_from;
    att.expires_at = data.expires_at;

    emit!(AttestationUpdated {
        issuer: att.issuer,
        subject: att.subject,
        schema_id: att.schema_id,
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
    require!(
        att.status == attestation_status::ACTIVE,
        AegisError::AlreadyRevoked
    );
    att.status = attestation_status::REVOKED;
    emit!(AttestationRevoked {
        issuer: att.issuer,
        subject: att.subject,
        schema_id: att.schema_id,
        reason_code,
    });
    Ok(())
}

/// Cryptographic erasure (GDPR): the issuer has destroyed the off-chain PII +
/// salt, rendering the on-chain commitment permanently unopenable. Distinct
/// from revoke (credential withdrawal) — both are terminal.
pub fn handle_erase_attestation(ctx: Context<ManageAttestation>) -> Result<()> {
    require!(
        ctx.accounts
            .issuer
            .is_signer_authorized(&ctx.accounts.signer.key()),
        AegisError::Unauthorized
    );
    let att = &mut ctx.accounts.attestation;
    att.status = attestation_status::ERASED;
    emit!(AttestationErased {
        issuer: att.issuer,
        subject: att.subject,
        schema_id: att.schema_id,
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
        seeds = [
            ATTESTATION_SEED,
            issuer.key().as_ref(),
            attestation.subject.as_ref(),
            &attestation.schema_id.to_le_bytes(),
        ],
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
        schema_id: ctx.accounts.attestation.schema_id,
    });
    Ok(())
}
