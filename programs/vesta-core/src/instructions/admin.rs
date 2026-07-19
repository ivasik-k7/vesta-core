use anchor_lang::prelude::*;

use crate::{
    constants::CONFIG_SEED,
    error::VestaError,
    events::{AdminChanged, AdminProposed, PausedSet},
    state::Config,
};

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ VestaError::Unauthorized,
    )]
    pub config: Account<'info, Config>,
}

pub fn handle_set_admin(ctx: Context<AdminOnly>, new_admin: Pubkey) -> Result<()> {
    let config = &mut ctx.accounts.config;
    config.pending_admin = Some(new_admin);

    emit!(AdminProposed {
        old: config.admin,
        new: new_admin,
    });
    Ok(())
}

pub fn handle_set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
    ctx.accounts.config.paused = paused;
    emit!(PausedSet { paused });
    Ok(())
}

#[derive(Accounts)]
pub struct AcceptAdmin<'info> {
    pub pending_admin: Signer<'info>,

    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
}

pub fn handle_accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
    let config = &mut ctx.accounts.config;
    require!(
        config.pending_admin == Some(ctx.accounts.pending_admin.key()),
        VestaError::PendingAdminMismatch
    );

    let old = config.admin;
    config.admin = ctx.accounts.pending_admin.key();
    config.pending_admin = None;

    emit!(AdminChanged {
        old,
        new: config.admin,
    });
    Ok(())
}
