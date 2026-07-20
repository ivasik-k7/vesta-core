//! Argus — the hundred-eyed guard of VESTA point transfers.
//!
//! Implements the SPL transfer-hook interface: Token-2022 CPIs `execute` on
//! every transfer of a hooked mint. v2 is a per-mint, merchant-configurable
//! policy engine (see docs/ARGUS_SPEC.md): a tunable GuardConfig drives the
//! decision pipeline — issuer/treasury short-circuits, per-mint pause,
//! allow/deny lists, aegis attestation gating, and a full velocity model
//! (per-tx cap, cooldown, count cap, balance cap, daily volume cap). Every
//! decision emits a reason-coded audit event.

pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;
use spl_discriminator::SplDiscriminate;
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx");

#[program]
pub mod argus {
    use super::*;

    /// Permissioned creation of the GuardConfig + ExtraAccountMetaList,
    /// seeded with an initial policy (spec §3.1).
    pub fn initialize_transfer_guard(
        ctx: Context<InitializeTransferGuard>,
        policy: InitialPolicy,
    ) -> Result<()> {
        instructions::initialize_transfer_guard::handle_initialize_transfer_guard(ctx, policy)
    }

    /// Retune the per-mint policy — guard authority only (spec §3.2).
    pub fn configure_policy(ctx: Context<GuardAuthorityOnly>, update: PolicyUpdate) -> Result<()> {
        instructions::admin::handle_configure_policy(ctx, update)
    }

    /// Per-mint peer-transfer circuit breaker — guard authority only (spec §3.3).
    pub fn set_guard_paused(ctx: Context<GuardAuthorityOnly>, paused: bool) -> Result<()> {
        instructions::admin::handle_set_guard_paused(ctx, paused)
    }

    /// Propose a new guard authority (two-step, spec §3.4).
    pub fn transfer_guard_authority(
        ctx: Context<GuardAuthorityOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        instructions::admin::handle_transfer_guard_authority(ctx, new_authority)
    }

    /// Accept a proposed guard authority (two-step, spec §3.4).
    pub fn accept_guard_authority(ctx: Context<AcceptGuardAuthority>) -> Result<()> {
        instructions::admin::handle_accept_guard_authority(ctx)
    }

    /// Add an allow/deny list member — guard authority only (spec §3.5).
    pub fn add_list_entry(ctx: Context<AddListEntry>, target: Pubkey) -> Result<()> {
        instructions::admin::handle_add_list_entry(ctx, target)
    }

    /// Remove an allow/deny list member — guard authority only (spec §3.5).
    pub fn remove_list_entry(ctx: Context<RemoveListEntry>, target: Pubkey) -> Result<()> {
        instructions::admin::handle_remove_list_entry(ctx, target)
    }

    /// One-time, customer-signed velocity-state creation — in-hook creation is
    /// impossible (privilege de-escalation leaves no rent payer, spec §3.6).
    pub fn open_wallet_state(ctx: Context<OpenWalletState>) -> Result<()> {
        instructions::open_wallet_state::handle_open_wallet_state(ctx)
    }

    /// Invoked by Token-2022 on every transfer of a hooked mint (spec §5).
    #[instruction(discriminator = ExecuteInstruction::SPL_DISCRIMINATOR_SLICE)]
    pub fn execute(ctx: Context<Execute>, amount: u64) -> Result<()> {
        instructions::execute::handle_execute(ctx, amount)
    }
}
