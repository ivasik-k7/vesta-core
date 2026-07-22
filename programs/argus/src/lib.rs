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

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "VESTA Argus — transfer-hook policy engine",
    project_url: "https://github.com/ivasik-k7/vesta-core",
    contacts: "email:kovtun.ivan@proton.me,link:https://github.com/ivasik-k7/vesta-core/blob/main/SECURITY.md",
    policy: "https://github.com/ivasik-k7/vesta-core/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/ivasik-k7/vesta-core",
    source_revision: "main",
    auditors: "None"
}

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

    /// Off-hot-path: cache a subject's aegis eligibility as a capability that
    /// `execute` reads with no CPI (spec 09). Permissionless; bundle before a
    /// transfer when the capability is missing or stale.
    pub fn refresh_eligibility(ctx: Context<RefreshEligibility>) -> Result<()> {
        instructions::refresh_eligibility::handle_refresh_eligibility(ctx)
    }

    /// Guard-authority: immediately invalidate a subject's cached capability
    /// (e.g. on a known aegis-side revocation), without a global epoch bump.
    pub fn invalidate_capability(ctx: Context<InvalidateCapability>) -> Result<()> {
        instructions::refresh_eligibility::handle_invalidate_capability(ctx)
    }

    // ── Governance (spec 10, phase 1) ────────────────────────────────────────

    /// Adopt the governed policy lifecycle for a mint (guard authority only).
    /// Seeds the role registry, active-policy pointer, and a genesis version
    /// capturing the current config; thereafter `configure_policy` is disabled
    /// and all changes run propose → approve → timelock → activate.
    pub fn initialize_governance(
        ctx: Context<InitializeGovernance>,
        genesis_hash: [u8; 32],
        roles: RoleAssignment,
        timelock_secs: i64,
    ) -> Result<()> {
        instructions::governance::handle_initialize_governance(
            ctx,
            genesis_hash,
            roles,
            timelock_secs,
        )
    }

    /// Propose an immutable, content-addressed policy version (Author role).
    pub fn propose_policy(
        ctx: Context<ProposePolicy>,
        policy_hash: [u8; 32],
        doc: PolicyDoc,
    ) -> Result<()> {
        instructions::governance::handle_propose_policy(ctx, policy_hash, doc)
    }

    /// Approve a pending version and start the timelock (Approver ≠ Author).
    pub fn approve_policy(ctx: Context<ApprovePolicy>) -> Result<()> {
        instructions::governance::handle_approve_policy(ctx)
    }

    /// Activate an approved version after the timelock (Activator role).
    pub fn activate_policy(ctx: Context<ActivatePolicy>) -> Result<()> {
        instructions::governance::handle_activate_policy(ctx)
    }

    /// Expedited re-point to any prior approved version (Activator role).
    pub fn rollback_policy(ctx: Context<ActivatePolicy>) -> Result<()> {
        instructions::governance::handle_rollback_policy(ctx)
    }

    /// Finalize the active version as immutable — freeze-only stays alive
    /// (RoleAdmin). Configurable immutability, not an all-or-nothing cliff.
    pub fn pin_policy(ctx: Context<PinPolicy>) -> Result<()> {
        instructions::governance::handle_pin_policy(ctx)
    }

    /// Queue a timelocked role reassignment (RoleAdmin).
    pub fn propose_role_change(
        ctx: Context<RoleChange>,
        role: u8,
        authority: Pubkey,
    ) -> Result<()> {
        instructions::governance::handle_propose_role_change(ctx, role, authority)
    }

    /// Apply a queued role reassignment after its timelock (RoleAdmin).
    pub fn apply_role_change(ctx: Context<RoleChange>) -> Result<()> {
        instructions::governance::handle_apply_role_change(ctx)
    }

    /// Anchor a period's decision-statement Merkle root on-chain (Reporter role,
    /// spec 10 phase 2) — tamper-evident and provably complete.
    pub fn anchor_statement(
        ctx: Context<AnchorStatement>,
        period: u64,
        merkle_root: [u8; 32],
        decision_count: u64,
    ) -> Result<()> {
        instructions::statements::handle_anchor_statement(ctx, period, merkle_root, decision_count)
    }

    // ── Trust triangle (spec 10, phase 3) ────────────────────────────────────

    /// Bind the mint's governing issuer to an aegis accreditation root (guard
    /// authority). Configures the fall-to posture and grace window.
    pub fn set_trust_anchor(
        ctx: Context<SetTrustAnchor>,
        accreditation_root: Pubkey,
        subject_issuer: Pubkey,
        required_schema: u64,
        degrade_target: u8,
        grace_secs: i64,
    ) -> Result<()> {
        instructions::trust::handle_set_trust_anchor(
            ctx,
            accreditation_root,
            subject_issuer,
            required_schema,
            degrade_target,
            grace_secs,
        )
    }

    /// Permissionless crank: re-check the governing issuer's aegis accreditation
    /// and auto-degrade (after grace) or auto-restore the transfer posture.
    pub fn reverify_accreditation(ctx: Context<ReverifyAccreditation>) -> Result<()> {
        instructions::trust::handle_reverify_accreditation(ctx)
    }

    /// Guard-authority manual posture override — emergency degrade, or restore
    /// to NORMAL after resolving a dispute (challenge path).
    pub fn set_degrade_mode(ctx: Context<SetDegradeMode>, mode: u8) -> Result<()> {
        instructions::trust::handle_set_degrade_mode(ctx, mode)
    }

    /// Advance the screening epoch (spec 10 phase 4 SANCTIONS): instantly stale
    /// every cached capability of the mint for near-real-time freeze propagation.
    pub fn bump_screening_epoch(ctx: Context<BumpScreeningEpoch>) -> Result<()> {
        instructions::trust::handle_bump_screening_epoch(ctx)
    }

    // ── Multi-tenancy & licensing (spec 10, phase 5) ─────────────────────────

    /// Trust-on-first-use creation of the protocol config + fee treasury.
    pub fn initialize_protocol(
        ctx: Context<InitializeProtocol>,
        license_fee_lamports: u64,
    ) -> Result<()> {
        instructions::licensing::handle_initialize_protocol(ctx, license_fee_lamports)
    }

    /// Set the per-period license fee (protocol authority).
    pub fn set_license_fee(ctx: Context<ProtocolAuthorityOnly>, fee: u64) -> Result<()> {
        instructions::licensing::handle_set_license_fee(ctx, fee)
    }

    /// Propose a new protocol authority (two-step).
    pub fn transfer_protocol_authority(
        ctx: Context<ProtocolAuthorityOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        instructions::licensing::handle_transfer_protocol_authority(ctx, new_authority)
    }

    /// Accept a proposed protocol authority (two-step).
    pub fn accept_protocol_authority(ctx: Context<AcceptProtocolAuthority>) -> Result<()> {
        instructions::licensing::handle_accept_protocol_authority(ctx)
    }

    /// Withdraw accrued license fees from the treasury (protocol authority).
    pub fn withdraw_fees(ctx: Context<WithdrawFees>, amount: u64) -> Result<()> {
        instructions::licensing::handle_withdraw_fees(ctx, amount)
    }

    /// Grant/update a mint's premium license terms (protocol authority).
    pub fn set_license(
        ctx: Context<SetLicense>,
        tier: u8,
        entitlements: u32,
        expires_at: i64,
    ) -> Result<()> {
        instructions::licensing::handle_set_license(ctx, tier, entitlements, expires_at)
    }

    /// Tenant pays the fee to extend their license by `periods` (spec 10 §4.7).
    pub fn purchase_license(ctx: Context<PurchaseLicense>, periods: u32) -> Result<()> {
        instructions::licensing::handle_purchase_license(ctx, periods)
    }

    /// Invoked by Token-2022 on every transfer of a hooked mint (spec §5).
    #[instruction(discriminator = ExecuteInstruction::SPL_DISCRIMINATOR_SLICE)]
    pub fn execute(ctx: Context<Execute>, amount: u64) -> Result<()> {
        instructions::execute::handle_execute(ctx, amount)
    }
}
