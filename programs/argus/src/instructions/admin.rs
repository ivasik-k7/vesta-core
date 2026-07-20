use anchor_lang::prelude::*;

use crate::{
    constants::GUARD_SEED,
    error::GuardError,
    events::{
        GuardAuthorityChanged, GuardAuthorityProposed, GuardPausedSet, ListEntryChanged,
        PolicyConfigured,
    },
    instructions::policy::{require_guard_authority, PolicyUpdate},
    state::{GuardConfig, PolicyListEntry},
};

/// Signer must be the current guard authority (spec §3.2–3.5).
#[derive(Accounts)]
pub struct GuardAuthorityOnly<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [GUARD_SEED, guard_config.mint.as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,
}

pub fn handle_configure_policy(
    ctx: Context<GuardAuthorityOnly>,
    update: PolicyUpdate,
) -> Result<()> {
    require_guard_authority(
        ctx.accounts.guard_config.authority,
        ctx.accounts.authority.key(),
    )?;

    let config = &mut ctx.accounts.guard_config;
    update.apply(config)?;

    emit!(PolicyConfigured {
        mint: config.mint,
        flags: config.flags,
        daily_gift_cap: config.daily_gift_cap,
        per_tx_cap: config.per_tx_cap,
        max_wallet_balance: config.max_wallet_balance,
        transfers_per_day_cap: config.transfers_per_day_cap,
        cooldown_secs: config.cooldown_secs,
        attestation_schema: config.attestation_schema,
        attestation_mask: config.attestation_mask,
    });
    Ok(())
}

pub fn handle_set_guard_paused(ctx: Context<GuardAuthorityOnly>, paused: bool) -> Result<()> {
    require_guard_authority(
        ctx.accounts.guard_config.authority,
        ctx.accounts.authority.key(),
    )?;
    let config = &mut ctx.accounts.guard_config;
    config.paused = paused;
    emit!(GuardPausedSet {
        mint: config.mint,
        paused,
    });
    Ok(())
}

pub fn handle_transfer_guard_authority(
    ctx: Context<GuardAuthorityOnly>,
    new_authority: Pubkey,
) -> Result<()> {
    require_guard_authority(
        ctx.accounts.guard_config.authority,
        ctx.accounts.authority.key(),
    )?;
    let config = &mut ctx.accounts.guard_config;
    config.pending_authority = Some(new_authority);
    emit!(GuardAuthorityProposed {
        mint: config.mint,
        old: config.authority,
        new: new_authority,
    });
    Ok(())
}

/// Signer must be the pending authority — completes the two-step handover.
#[derive(Accounts)]
pub struct AcceptGuardAuthority<'info> {
    pub pending_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [GUARD_SEED, guard_config.mint.as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,
}

pub fn handle_accept_guard_authority(ctx: Context<AcceptGuardAuthority>) -> Result<()> {
    let config = &mut ctx.accounts.guard_config;
    require!(
        config.pending_authority == Some(ctx.accounts.pending_authority.key()),
        GuardError::PendingAuthorityMismatch
    );
    let old = config.authority;
    config.authority = ctx.accounts.pending_authority.key();
    config.pending_authority = None;
    emit!(GuardAuthorityChanged {
        mint: config.mint,
        old,
        new: config.authority,
    });
    Ok(())
}

/// Add a member to the allow/deny list — one PDA per target (spec §2.4, §3.5).
#[derive(Accounts)]
#[instruction(target: Pubkey)]
pub struct AddListEntry<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [GUARD_SEED, guard_config.mint.as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,

    #[account(
        init,
        payer = authority,
        space = 8 + PolicyListEntry::INIT_SPACE,
        seeds = [crate::constants::LIST_ENTRY_SEED, guard_config.mint.as_ref(), target.as_ref()],
        bump,
    )]
    pub entry: Account<'info, PolicyListEntry>,

    pub system_program: Program<'info, System>,
}

pub fn handle_add_list_entry(ctx: Context<AddListEntry>, target: Pubkey) -> Result<()> {
    require_guard_authority(
        ctx.accounts.guard_config.authority,
        ctx.accounts.authority.key(),
    )?;
    let entry = &mut ctx.accounts.entry;
    entry.mint = ctx.accounts.guard_config.mint;
    entry.target = target;
    entry.bump = ctx.bumps.entry;
    emit!(ListEntryChanged {
        mint: ctx.accounts.guard_config.mint,
        target,
        added: true,
    });
    Ok(())
}

/// Remove a member — reclaims the entry's rent to the authority.
#[derive(Accounts)]
#[instruction(target: Pubkey)]
pub struct RemoveListEntry<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [GUARD_SEED, guard_config.mint.as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,

    #[account(
        mut,
        close = authority,
        seeds = [crate::constants::LIST_ENTRY_SEED, guard_config.mint.as_ref(), target.as_ref()],
        bump = entry.bump,
    )]
    pub entry: Account<'info, PolicyListEntry>,
}

pub fn handle_remove_list_entry(ctx: Context<RemoveListEntry>, target: Pubkey) -> Result<()> {
    require_guard_authority(
        ctx.accounts.guard_config.authority,
        ctx.accounts.authority.key(),
    )?;
    emit!(ListEntryChanged {
        mint: ctx.accounts.guard_config.mint,
        target,
        added: false,
    });
    Ok(())
}
