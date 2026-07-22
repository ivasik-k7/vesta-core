//! Governed policy lifecycle + separation-of-duties roles (spec 10, phase 1).
//!
//! Replaces silent live-mutation of `GuardConfig` with an auditable lifecycle:
//! `propose → approve (≠ author) → timelock → activate`, plus expedited
//! `rollback` and configurable-immutability `pin`. Every privileged step checks
//! a *specific* role in the per-mint `RoleRegistry`, so a compromised operator
//! of one kind cannot exercise another's power. Governance is opt-in and
//! additive — a mint that never calls `initialize_governance` keeps the
//! free-tier single-authority `configure_policy` path unchanged.

use anchor_lang::prelude::*;

use crate::{
    constants::{
        GUARD_SEED, MAX_GOVERNANCE_TIMELOCK_SECS, POLICY_POINTER_SEED, POLICY_VERSION_SEED,
        ROLES_SEED, STATE_VERSION,
    },
    error::GuardError,
    events::{
        GovernanceInitialized, PolicyActivated, PolicyApproved, PolicyPinned, PolicyProposed,
        RoleChangeProposed, RoleChanged,
    },
    instructions::policy::validate_policy,
    state::{GuardConfig, PolicyDoc, PolicyPointer, PolicyVersion, Role, RoleRegistry},
};

/// All six role authorities assigned when a mint adopts governance. Any key may
/// be a multisig (e.g. a Squads PDA); argus treats it as one signer. Author and
/// Approver SHOULD be distinct — approval enforces `approver != author`, so a
/// mint that sets them equal can never activate a change.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct RoleAssignment {
    pub role_admin: Pubkey,
    pub author: Pubkey,
    pub approver: Pubkey,
    pub activator: Pubkey,
    pub pause_operator: Pubkey,
    pub reporter: Pubkey,
}

// ── initialize_governance ────────────────────────────────────────────────────

#[derive(Accounts)]
#[instruction(genesis_hash: [u8; 32])]
pub struct InitializeGovernance<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: the hooked mint — PDA seed only.
    pub mint: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [GUARD_SEED, mint.key().as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,

    #[account(
        init,
        payer = authority,
        space = 8 + RoleRegistry::INIT_SPACE,
        seeds = [ROLES_SEED, mint.key().as_ref()],
        bump,
    )]
    pub role_registry: Account<'info, RoleRegistry>,

    #[account(
        init,
        payer = authority,
        space = 8 + PolicyPointer::INIT_SPACE,
        seeds = [POLICY_POINTER_SEED, mint.key().as_ref()],
        bump,
    )]
    pub policy_pointer: Account<'info, PolicyPointer>,

    /// The genesis version — captures the current `GuardConfig` as the first
    /// immutable, content-addressed policy (already active, no timelock).
    #[account(
        init,
        payer = authority,
        space = 8 + PolicyVersion::INIT_SPACE,
        seeds = [POLICY_VERSION_SEED, mint.key().as_ref(), &genesis_hash],
        bump,
    )]
    pub genesis_version: Account<'info, PolicyVersion>,

    pub system_program: Program<'info, System>,
}

/// `genesis_hash` is supplied by the client and verified in-handler against the
/// hash of the current config so the PDA seed and the content agree.
pub fn handle_initialize_governance(
    ctx: Context<InitializeGovernance>,
    genesis_hash: [u8; 32],
    roles: RoleAssignment,
    timelock_secs: i64,
) -> Result<()> {
    let config = &mut ctx.accounts.guard_config;
    require_keys_eq!(
        config.authority,
        ctx.accounts.authority.key(),
        GuardError::Unauthorized
    );
    require!(!config.governed, GuardError::PolicyPinned);
    require!(
        (0..=MAX_GOVERNANCE_TIMELOCK_SECS).contains(&timelock_secs),
        GuardError::InvalidTimelock
    );
    require!(
        roles.role_admin != Pubkey::default(),
        GuardError::RoleUnauthorized
    );

    let doc = config.as_policy_doc();
    require!(doc.hash()? == genesis_hash, GuardError::PolicyHashMismatch);

    let now = Clock::get()?.unix_timestamp;
    let mint = ctx.accounts.mint.key();

    let genesis = &mut ctx.accounts.genesis_version;
    genesis.version = STATE_VERSION;
    genesis.mint = mint;
    genesis.policy_hash = genesis_hash;
    genesis.doc = doc;
    genesis.author = ctx.accounts.authority.key();
    genesis.approver = ctx.accounts.authority.key();
    genesis.proposed_at = now;
    genesis.approved_at = now;
    genesis.bump = ctx.bumps.genesis_version;

    let registry = &mut ctx.accounts.role_registry;
    registry.version = STATE_VERSION;
    registry.mint = mint;
    registry.role_admin = roles.role_admin;
    registry.author = roles.author;
    registry.approver = roles.approver;
    registry.activator = roles.activator;
    registry.pause_operator = roles.pause_operator;
    registry.reporter = roles.reporter;
    registry.pending_role = 0;
    registry.pending_authority = Pubkey::default();
    registry.pending_after = 0;
    registry.bump = ctx.bumps.role_registry;

    let pointer = &mut ctx.accounts.policy_pointer;
    pointer.version = STATE_VERSION;
    pointer.mint = mint;
    pointer.active_hash = genesis_hash;
    pointer.pending_hash = [0u8; 32];
    pointer.pending_approved_at = 0;
    pointer.timelock_secs = timelock_secs;
    pointer.pinned = false;
    pointer.bump = ctx.bumps.policy_pointer;

    config.governed = true;

    emit!(GovernanceInitialized {
        mint,
        role_admin: roles.role_admin,
        genesis_policy_hash: genesis_hash,
        timelock_secs,
    });
    Ok(())
}

// ── propose_policy ───────────────────────────────────────────────────────────

#[derive(Accounts)]
#[instruction(policy_hash: [u8; 32])]
pub struct ProposePolicy<'info> {
    #[account(mut)]
    pub author: Signer<'info>,

    #[account(
        seeds = [ROLES_SEED, policy_pointer.mint.as_ref()],
        bump = role_registry.bump,
    )]
    pub role_registry: Account<'info, RoleRegistry>,

    #[account(
        mut,
        seeds = [POLICY_POINTER_SEED, policy_pointer.mint.as_ref()],
        bump = policy_pointer.bump,
    )]
    pub policy_pointer: Account<'info, PolicyPointer>,

    #[account(
        init,
        payer = author,
        space = 8 + PolicyVersion::INIT_SPACE,
        seeds = [POLICY_VERSION_SEED, policy_pointer.mint.as_ref(), &policy_hash],
        bump,
    )]
    pub policy_version: Account<'info, PolicyVersion>,

    pub system_program: Program<'info, System>,
}

pub fn handle_propose_policy(
    ctx: Context<ProposePolicy>,
    policy_hash: [u8; 32],
    doc: PolicyDoc,
) -> Result<()> {
    let registry = &ctx.accounts.role_registry;
    registry.require(Role::Author, ctx.accounts.author.key())?;
    require!(
        !ctx.accounts.policy_pointer.pinned,
        GuardError::PolicyPinned
    );
    require!(doc.hash()? == policy_hash, GuardError::PolicyHashMismatch);

    let now = Clock::get()?.unix_timestamp;
    let mint = ctx.accounts.policy_pointer.mint;

    let version = &mut ctx.accounts.policy_version;
    version.version = STATE_VERSION;
    version.mint = mint;
    version.policy_hash = policy_hash;
    version.doc = doc;
    version.author = ctx.accounts.author.key();
    version.approver = Pubkey::default();
    version.proposed_at = now;
    version.approved_at = 0;
    version.bump = ctx.bumps.policy_version;

    let pointer = &mut ctx.accounts.policy_pointer;
    pointer.pending_hash = policy_hash;
    pointer.pending_approved_at = 0;

    emit!(PolicyProposed {
        mint,
        policy_hash,
        author: ctx.accounts.author.key(),
    });
    Ok(())
}

// ── approve_policy ───────────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct ApprovePolicy<'info> {
    pub approver: Signer<'info>,

    #[account(
        seeds = [ROLES_SEED, policy_pointer.mint.as_ref()],
        bump = role_registry.bump,
    )]
    pub role_registry: Account<'info, RoleRegistry>,

    #[account(
        mut,
        seeds = [POLICY_POINTER_SEED, policy_pointer.mint.as_ref()],
        bump = policy_pointer.bump,
    )]
    pub policy_pointer: Account<'info, PolicyPointer>,

    #[account(
        mut,
        seeds = [POLICY_VERSION_SEED, policy_pointer.mint.as_ref(), &policy_version.policy_hash],
        bump = policy_version.bump,
    )]
    pub policy_version: Account<'info, PolicyVersion>,
}

pub fn handle_approve_policy(ctx: Context<ApprovePolicy>) -> Result<()> {
    let registry = &ctx.accounts.role_registry;
    registry.require(Role::Approver, ctx.accounts.approver.key())?;

    let pointer = &ctx.accounts.policy_pointer;
    require!(!pointer.pinned, GuardError::PolicyPinned);
    require!(
        pointer.pending_hash == ctx.accounts.policy_version.policy_hash,
        GuardError::PolicyVersionMismatch
    );
    // Separation of duties: an approver may not rubber-stamp their own proposal.
    require_keys_neq!(
        ctx.accounts.approver.key(),
        ctx.accounts.policy_version.author,
        GuardError::SelfApproval
    );

    let now = Clock::get()?.unix_timestamp;
    let activate_after = now
        .checked_add(pointer.timelock_secs)
        .ok_or(GuardError::Overflow)?;
    let mint = pointer.mint;
    let policy_hash = ctx.accounts.policy_version.policy_hash;

    ctx.accounts.policy_version.approver = ctx.accounts.approver.key();
    ctx.accounts.policy_version.approved_at = now;
    ctx.accounts.policy_pointer.pending_approved_at = now;

    emit!(PolicyApproved {
        mint,
        policy_hash,
        approver: ctx.accounts.approver.key(),
        activate_after,
    });
    Ok(())
}

// ── activate_policy / rollback_policy ────────────────────────────────────────

#[derive(Accounts)]
pub struct ActivatePolicy<'info> {
    pub activator: Signer<'info>,

    #[account(
        seeds = [ROLES_SEED, guard_config.mint.as_ref()],
        bump = role_registry.bump,
    )]
    pub role_registry: Account<'info, RoleRegistry>,

    #[account(
        mut,
        seeds = [POLICY_POINTER_SEED, guard_config.mint.as_ref()],
        bump = policy_pointer.bump,
    )]
    pub policy_pointer: Account<'info, PolicyPointer>,

    #[account(
        seeds = [POLICY_VERSION_SEED, guard_config.mint.as_ref(), &policy_version.policy_hash],
        bump = policy_version.bump,
    )]
    pub policy_version: Account<'info, PolicyVersion>,

    #[account(
        mut,
        seeds = [GUARD_SEED, guard_config.mint.as_ref()],
        bump = guard_config.bump,
    )]
    pub guard_config: Account<'info, GuardConfig>,
}

pub fn handle_activate_policy(ctx: Context<ActivatePolicy>) -> Result<()> {
    let registry = &ctx.accounts.role_registry;
    registry.require(Role::Activator, ctx.accounts.activator.key())?;

    let pointer = &ctx.accounts.policy_pointer;
    let version = &ctx.accounts.policy_version;
    require!(!pointer.pinned, GuardError::PolicyPinned);
    require!(
        pointer.pending_hash == version.policy_hash,
        GuardError::PolicyVersionMismatch
    );
    require!(
        pointer.pending_approved_at != 0,
        GuardError::PolicyNotApproved
    );
    let now = Clock::get()?.unix_timestamp;
    let ready_at = pointer
        .pending_approved_at
        .checked_add(pointer.timelock_secs)
        .ok_or(GuardError::Overflow)?;
    require!(now >= ready_at, GuardError::TimelockActive);

    apply_and_repoint(
        &mut ctx.accounts.guard_config,
        &mut ctx.accounts.policy_pointer,
        &ctx.accounts.policy_version.doc,
        version.policy_hash,
        false,
    )
}

pub fn handle_rollback_policy(ctx: Context<ActivatePolicy>) -> Result<()> {
    let registry = &ctx.accounts.role_registry;
    registry.require(Role::Activator, ctx.accounts.activator.key())?;

    let pointer = &ctx.accounts.policy_pointer;
    let version = &ctx.accounts.policy_version;
    require!(!pointer.pinned, GuardError::PolicyPinned);
    // Expedited (no timelock), but only to a previously *approved* version.
    // Non-genesis versions already passed the `approver != author` SoD gate at
    // approval time; genesis is intentionally self-approved by the guard owner.
    require!(version.approved_at != 0, GuardError::PolicyNotApproved);

    let hash = version.policy_hash;
    apply_and_repoint(
        &mut ctx.accounts.guard_config,
        &mut ctx.accounts.policy_pointer,
        &ctx.accounts.policy_version.doc,
        hash,
        true,
    )
}

/// Write a `PolicyDoc` onto the live `GuardConfig`, bump the epoch (invalidating
/// stale capabilities, spec 09 §4.4), repoint the pointer, and clear any pending
/// change. Shared by activate and rollback.
fn apply_and_repoint(
    config: &mut Account<GuardConfig>,
    pointer: &mut Account<PolicyPointer>,
    doc: &PolicyDoc,
    new_hash: [u8; 32],
    rollback: bool,
) -> Result<()> {
    require!(doc.capability_ttl_secs >= 0, GuardError::InvalidPolicy);
    validate_policy(
        doc.flags,
        doc.daily_gift_cap,
        doc.per_tx_cap,
        config.aegis_program,
        doc.policy,
        config.attestation_issuer,
    )?;

    config.flags = doc.flags;
    config.daily_gift_cap = doc.daily_gift_cap;
    config.per_tx_cap = doc.per_tx_cap;
    config.max_wallet_balance = doc.max_wallet_balance;
    config.transfers_per_day_cap = doc.transfers_per_day_cap;
    config.cooldown_secs = doc.cooldown_secs;
    config.attestation_schema = doc.attestation_schema;
    config.capability_ttl_secs = doc.capability_ttl_secs;
    config.policy = doc.policy;
    config.policy_epoch = config.policy_epoch.saturating_add(1);

    let old_hash = pointer.active_hash;
    pointer.active_hash = new_hash;
    pointer.pending_hash = [0u8; 32];
    pointer.pending_approved_at = 0;

    emit!(PolicyActivated {
        mint: config.mint,
        old_hash,
        new_hash,
        policy_epoch: config.policy_epoch,
        rollback,
    });
    Ok(())
}

// ── pin_policy ───────────────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct PinPolicy<'info> {
    pub role_admin: Signer<'info>,

    #[account(
        seeds = [ROLES_SEED, policy_pointer.mint.as_ref()],
        bump = role_registry.bump,
    )]
    pub role_registry: Account<'info, RoleRegistry>,

    #[account(
        mut,
        seeds = [POLICY_POINTER_SEED, policy_pointer.mint.as_ref()],
        bump = policy_pointer.bump,
    )]
    pub policy_pointer: Account<'info, PolicyPointer>,
}

pub fn handle_pin_policy(ctx: Context<PinPolicy>) -> Result<()> {
    let registry = &ctx.accounts.role_registry;
    registry.require(Role::RoleAdmin, ctx.accounts.role_admin.key())?;

    let pointer = &mut ctx.accounts.policy_pointer;
    let hash = pointer.active_hash;
    pointer.pinned = true;

    emit!(PolicyPinned {
        mint: pointer.mint,
        policy_hash: hash,
    });
    Ok(())
}

// ── role changes (two-step, timelocked) ──────────────────────────────────────

#[derive(Accounts)]
pub struct RoleChange<'info> {
    pub role_admin: Signer<'info>,

    #[account(
        mut,
        seeds = [ROLES_SEED, role_registry.mint.as_ref()],
        bump = role_registry.bump,
    )]
    pub role_registry: Account<'info, RoleRegistry>,

    #[account(
        seeds = [POLICY_POINTER_SEED, role_registry.mint.as_ref()],
        bump = policy_pointer.bump,
    )]
    pub policy_pointer: Account<'info, PolicyPointer>,
}

pub fn handle_propose_role_change(
    ctx: Context<RoleChange>,
    role: u8,
    authority: Pubkey,
) -> Result<()> {
    let admin_key = ctx.accounts.role_admin.key();
    ctx.accounts
        .role_registry
        .require(Role::RoleAdmin, admin_key)?;
    let target = Role::try_from(role)?;
    // A role must always resolve to a real key (fail-closed `require`); refuse to
    // queue an unset RoleAdmin, which would strand governance with no admin.
    if matches!(target, Role::RoleAdmin) {
        require_keys_neq!(authority, Pubkey::default(), GuardError::RoleUnauthorized);
    }

    let now = Clock::get()?.unix_timestamp;
    let apply_after = now
        .checked_add(ctx.accounts.policy_pointer.timelock_secs)
        .ok_or(GuardError::Overflow)?;
    let mint = ctx.accounts.role_registry.mint;

    let registry = &mut ctx.accounts.role_registry;
    registry.pending_role = role;
    registry.pending_authority = authority;
    registry.pending_after = apply_after;

    emit!(RoleChangeProposed {
        mint,
        role,
        authority,
        apply_after,
    });
    Ok(())
}

pub fn handle_apply_role_change(ctx: Context<RoleChange>) -> Result<()> {
    let admin_key = ctx.accounts.role_admin.key();
    ctx.accounts
        .role_registry
        .require(Role::RoleAdmin, admin_key)?;

    let registry = &ctx.accounts.role_registry;
    require!(registry.pending_after != 0, GuardError::NoPendingChange);
    let now = Clock::get()?.unix_timestamp;
    require!(now >= registry.pending_after, GuardError::TimelockActive);

    let role = Role::try_from(registry.pending_role)?;
    let new = registry.pending_authority;
    let mint = registry.mint;
    let old = registry.authority_for(role);

    let registry = &mut ctx.accounts.role_registry;
    registry.set(role, new);
    registry.pending_role = 0;
    registry.pending_authority = Pubkey::default();
    registry.pending_after = 0;

    emit!(RoleChanged {
        mint,
        role: role as u8,
        old,
        new,
    });
    Ok(())
}
