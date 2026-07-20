use anchor_lang::prelude::*;

use crate::{
    constants::{ISSUER_SEED, MAX_NAME_LEN},
    error::AegisError,
    events::{
        IssuerAuthorityChanged, IssuerAuthorityProposed, IssuerInitialized, IssuerPausedSet,
    },
    state::Issuer,
};

#[derive(Accounts)]
pub struct InitIssuer<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + Issuer::INIT_SPACE,
        seeds = [ISSUER_SEED, authority.key().as_ref()],
        bump,
    )]
    pub issuer: Account<'info, Issuer>,

    pub system_program: Program<'info, System>,
}

pub fn handle_init_issuer(ctx: Context<InitIssuer>, name: String) -> Result<()> {
    require!(name.len() <= MAX_NAME_LEN, AegisError::NameTooLong);
    let issuer = &mut ctx.accounts.issuer;
    issuer.authority = ctx.accounts.authority.key();
    issuer.pending_authority = None;
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

/// Issuer-authority-gated context. The issuer is referenced by address (not
/// re-derived), so authority rotation does not orphan it.
#[derive(Accounts)]
pub struct IssuerAuthorityOnly<'info> {
    pub authority: Signer<'info>,

    #[account(mut, has_one = authority @ AegisError::Unauthorized)]
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
    emit!(IssuerAuthorityChanged {
        issuer: issuer.key(),
        old,
        new: issuer.authority,
    });
    Ok(())
}
