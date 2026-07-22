use anchor_lang::prelude::*;

use crate::{
    constants::{ISSUER_SEED, MAX_NAME_LEN, STATE_VERSION},
    error::AegisError,
    events::{
        IssuerAuthorityChanged, IssuerAuthorityProposed, IssuerInitialized, IssuerOperatorSet,
        IssuerPausedSet,
    },
    state::Issuer,
};

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct InitIssuer<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + Issuer::INIT_SPACE,
        seeds = [ISSUER_SEED, authority.key().as_ref(), &id.to_le_bytes()],
        bump,
    )]
    pub issuer: Account<'info, Issuer>,

    pub system_program: Program<'info, System>,
}

pub fn handle_init_issuer(ctx: Context<InitIssuer>, id: u64, name: String) -> Result<()> {
    require!(
        !name.is_empty() && name.len() <= MAX_NAME_LEN,
        AegisError::InvalidName
    );
    let issuer = &mut ctx.accounts.issuer;
    issuer.version = STATE_VERSION;
    issuer.id = id;
    issuer.authority = ctx.accounts.authority.key();
    issuer.pending_authority = None;
    issuer.operator = None;
    issuer.name = name;
    issuer.issued = 0;
    issuer.paused = false;
    issuer.bump = ctx.bumps.issuer;

    emit!(IssuerInitialized {
        issuer: issuer.key(),
        authority: issuer.authority,
    });
    Ok(())
}

/// Cold-admin-gated context: the signer MUST be the issuer authority (not the
/// operator). Used for pause, operator management, and authority rotation.
#[derive(Accounts)]
pub struct IssuerAuthorityOnly<'info> {
    pub authority: Signer<'info>,

    #[account(mut, has_one = authority @ AegisError::AuthorityOnly)]
    pub issuer: Account<'info, Issuer>,
}

pub fn handle_set_issuer_paused(ctx: Context<IssuerAuthorityOnly>, paused: bool) -> Result<()> {
    let issuer = &mut ctx.accounts.issuer;
    issuer.paused = paused;
    emit!(IssuerPausedSet {
        issuer: issuer.key(),
        paused,
    });
    Ok(())
}

pub fn handle_set_operator(
    ctx: Context<IssuerAuthorityOnly>,
    operator: Option<Pubkey>,
) -> Result<()> {
    let issuer = &mut ctx.accounts.issuer;
    issuer.operator = operator;
    emit!(IssuerOperatorSet {
        issuer: issuer.key(),
        operator,
    });
    Ok(())
}

pub fn handle_transfer_issuer_authority(
    ctx: Context<IssuerAuthorityOnly>,
    new_authority: Pubkey,
) -> Result<()> {
    let issuer = &mut ctx.accounts.issuer;
    issuer.pending_authority = Some(new_authority);
    emit!(IssuerAuthorityProposed {
        issuer: issuer.key(),
        old: issuer.authority,
        new: new_authority,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct AcceptIssuerAuthority<'info> {
    pub pending_authority: Signer<'info>,

    #[account(mut)]
    pub issuer: Account<'info, Issuer>,
}

pub fn handle_accept_issuer_authority(ctx: Context<AcceptIssuerAuthority>) -> Result<()> {
    let issuer = &mut ctx.accounts.issuer;
    require!(
        issuer.pending_authority == Some(ctx.accounts.pending_authority.key()),
        AegisError::PendingAuthorityMismatch
    );
    let old = issuer.authority;
    issuer.authority = ctx.accounts.pending_authority.key();
    issuer.pending_authority = None;
    // A new authority starts with a clean operator slot — the old hot key,
    // provisioned by the previous admin, must be re-granted explicitly.
    issuer.operator = None;
    emit!(IssuerAuthorityChanged {
        issuer: issuer.key(),
        old,
        new: issuer.authority,
    });
    Ok(())
}
