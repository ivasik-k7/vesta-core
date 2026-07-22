use anchor_lang::prelude::*;

use crate::{
    constants::{
        verify_reason, ACCREDITATION_SEED, MAX_NAME_LEN, MAX_PERMITTED_SCHEMAS, STATE_VERSION,
        TRUST_ROOT_SEED,
    },
    error::AegisError,
    events::{AccreditationRevoked, IssuerAccredited, TrustRootActiveSet, TrustRootRegistered},
    instructions::verify::{emit_verdict, Verdict},
    state::{accreditation_status, Accreditation, TrustRoot},
};

// ── Trust root ───────────────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct RegisterTrustRoot<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + TrustRoot::INIT_SPACE,
        seeds = [TRUST_ROOT_SEED, authority.key().as_ref()],
        bump,
    )]
    pub trust_root: Account<'info, TrustRoot>,

    pub system_program: Program<'info, System>,
}

pub fn handle_register_trust_root(ctx: Context<RegisterTrustRoot>, name: String) -> Result<()> {
    require!(
        !name.is_empty() && name.len() <= MAX_NAME_LEN,
        AegisError::InvalidName
    );
    let root = &mut ctx.accounts.trust_root;
    root.version = STATE_VERSION;
    root.authority = ctx.accounts.authority.key();
    root.name = name;
    root.active = true;
    root.bump = ctx.bumps.trust_root;

    emit!(TrustRootRegistered {
        root: root.key(),
        authority: root.authority,
    });
    Ok(())
}

/// Enable/disable a trust root — the atomic incident-response kill-switch. When
/// inactive, `verify_accreditation` fails closed for EVERY issuer under the root
/// in one action (vs. revoking each edge individually).
#[derive(Accounts)]
pub struct SetRootActive<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority @ AegisError::Unauthorized,
        seeds = [TRUST_ROOT_SEED, authority.key().as_ref()],
        bump = trust_root.bump,
    )]
    pub trust_root: Account<'info, TrustRoot>,
}

pub fn handle_set_root_active(ctx: Context<SetRootActive>, active: bool) -> Result<()> {
    let root = &mut ctx.accounts.trust_root;
    root.active = active;
    emit!(TrustRootActiveSet {
        root: root.key(),
        active,
    });
    Ok(())
}

// ── Accreditation (root → issuer) ────────────────────────────────────────────

#[derive(Accounts)]
#[instruction(subject_issuer: Pubkey)]
pub struct AccreditIssuer<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        has_one = authority @ AegisError::Unauthorized,
        seeds = [TRUST_ROOT_SEED, authority.key().as_ref()],
        bump = trust_root.bump,
    )]
    pub trust_root: Account<'info, TrustRoot>,

    #[account(
        init,
        payer = authority,
        space = 8 + Accreditation::INIT_SPACE,
        seeds = [ACCREDITATION_SEED, authority.key().as_ref(), subject_issuer.as_ref()],
        bump,
    )]
    pub accreditation: Account<'info, Accreditation>,

    pub system_program: Program<'info, System>,
}

pub fn handle_accredit_issuer(
    ctx: Context<AccreditIssuer>,
    subject_issuer: Pubkey,
    tier: u8,
    permitted_schemas: Vec<u64>,
    jurisdiction: u16,
    expires_at: i64,
) -> Result<()> {
    require!(ctx.accounts.trust_root.active, AegisError::Unauthorized);
    // Least-privilege: an accreditation must name at least one schema and no
    // more than the cap — there is no "all schemas" wildcard.
    require!(
        !permitted_schemas.is_empty(),
        AegisError::PermittedSchemasRequired
    );
    require!(
        permitted_schemas.len() <= MAX_PERMITTED_SCHEMAS,
        AegisError::StringTooLong
    );
    let now = Clock::get()?.unix_timestamp;
    if expires_at != 0 {
        require!(expires_at > now, AegisError::InvalidExpiry);
    }

    let acc = &mut ctx.accounts.accreditation;
    acc.version = STATE_VERSION;
    acc.root = ctx.accounts.authority.key();
    acc.subject_issuer = subject_issuer;
    acc.tier = tier;
    acc.permitted_schemas = [0u64; MAX_PERMITTED_SCHEMAS];
    for (slot, schema) in acc
        .permitted_schemas
        .iter_mut()
        .zip(permitted_schemas.iter())
    {
        *slot = *schema;
    }
    acc.permitted_count =
        u8::try_from(permitted_schemas.len()).map_err(|_| AegisError::StringTooLong)?;
    acc.jurisdiction = jurisdiction;
    acc.status = accreditation_status::ACTIVE;
    acc.issued_at = now;
    acc.expires_at = expires_at;
    acc.bump = ctx.bumps.accreditation;

    emit!(IssuerAccredited {
        root: acc.root,
        subject_issuer,
        tier,
        jurisdiction,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct RevokeAccreditation<'info> {
    pub authority: Signer<'info>,

    #[account(
        has_one = authority @ AegisError::Unauthorized,
        seeds = [TRUST_ROOT_SEED, authority.key().as_ref()],
        bump = trust_root.bump,
    )]
    pub trust_root: Account<'info, TrustRoot>,

    #[account(
        mut,
        seeds = [ACCREDITATION_SEED, authority.key().as_ref(), accreditation.subject_issuer.as_ref()],
        bump = accreditation.bump,
    )]
    pub accreditation: Account<'info, Accreditation>,
}

pub fn handle_revoke_accreditation(ctx: Context<RevokeAccreditation>) -> Result<()> {
    let acc = &mut ctx.accounts.accreditation;
    acc.status = accreditation_status::REVOKED;
    emit!(AccreditationRevoked {
        root: acc.root,
        subject_issuer: acc.subject_issuer,
    });
    Ok(())
}

// ── verify_accreditation (verdict primitive) ─────────────────────────────────

#[derive(Accounts)]
pub struct VerifyAccreditation<'info> {
    /// CHECK: TrustRoot PDA; re-derived + owner-checked in the handler.
    pub trust_root: UncheckedAccount<'info>,
    /// CHECK: Accreditation PDA; re-derived + owner-checked in the handler.
    pub accreditation: UncheckedAccount<'info>,
}

/// Stateless verdict: is `subject_issuer` accredited by trust root `root` for
/// `schema_id`? Fails closed (never reverts) — returns a `Verdict` via
/// return-data so callers compose it with a credential `verify`.
pub fn handle_verify_accreditation(
    ctx: Context<VerifyAccreditation>,
    root: Pubkey,
    subject_issuer: Pubkey,
    schema_id: u64,
) -> Result<()> {
    let verdict = evaluate(
        &ctx.accounts.trust_root,
        &ctx.accounts.accreditation,
        root,
        subject_issuer,
        schema_id,
    );
    emit_verdict(&verdict)
}

fn fail(reason: u16) -> Verdict {
    Verdict {
        ok: false,
        reason_code: reason,
        ..Verdict::default()
    }
}

fn evaluate(
    trust_root: &UncheckedAccount,
    accreditation: &UncheckedAccount,
    root: Pubkey,
    subject_issuer: Pubkey,
    schema_id: u64,
) -> Verdict {
    // Trust root must be the canonical PDA, aegis-owned, and active.
    let expected_root =
        Pubkey::find_program_address(&[TRUST_ROOT_SEED, root.as_ref()], &crate::ID).0;
    if trust_root.key() != expected_root
        || trust_root.owner != &crate::ID
        || trust_root.data_is_empty()
    {
        return fail(verify_reason::ROOT_INACTIVE);
    }
    match trust_root
        .try_borrow_data()
        .ok()
        .and_then(|d| TrustRoot::try_deserialize(&mut d.as_ref()).ok())
    {
        Some(tr) if tr.version == STATE_VERSION && tr.active => {}
        _ => return fail(verify_reason::ROOT_INACTIVE),
    }

    // Accreditation edge must be the canonical PDA for (root, subject_issuer).
    let expected_acc = Pubkey::find_program_address(
        &[ACCREDITATION_SEED, root.as_ref(), subject_issuer.as_ref()],
        &crate::ID,
    )
    .0;
    if accreditation.key() != expected_acc
        || accreditation.owner != &crate::ID
        || accreditation.data_is_empty()
    {
        return fail(verify_reason::NOT_ACCREDITED);
    }
    let acc = match accreditation
        .try_borrow_data()
        .ok()
        .and_then(|d| Accreditation::try_deserialize(&mut d.as_ref()).ok())
    {
        Some(a) => a,
        None => return fail(verify_reason::NOT_ACCREDITED),
    };
    let now = match Clock::get() {
        Ok(c) => c.unix_timestamp,
        Err(_) => return fail(verify_reason::NOT_ACCREDITED),
    };
    if acc.version != STATE_VERSION
        || acc.root != root
        || acc.subject_issuer != subject_issuer
        || !acc.is_live(now)
    {
        return fail(verify_reason::NOT_ACCREDITED);
    }
    if !acc.permits(schema_id) {
        return fail(verify_reason::SCHEMA_NOT_PERMITTED);
    }

    Verdict {
        ok: true,
        reason_code: verify_reason::OK,
        issuer: subject_issuer,
        schema_id,
        expires_at: acc.expires_at,
        jurisdiction: acc.jurisdiction,
        tier: acc.tier,
    }
}
