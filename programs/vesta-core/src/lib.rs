pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod state;
pub(crate) mod util;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz");

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "VESTA Core — living loyalty protocol",
    project_url: "https://github.com/ivasik-k7/vesta-core",
    contacts: "email:kovtun.ivan@proton.me,link:https://github.com/ivasik-k7/vesta-core/blob/main/SECURITY.md",
    policy: "https://github.com/ivasik-k7/vesta-core/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/ivasik-k7/vesta-core",
    source_revision: "main",
    auditors: "None"
}

#[program]
pub mod vesta_core {
    use super::*;

    pub fn init_config(ctx: Context<InitConfig>) -> Result<()> {
        instructions::init_config::handle_init_config(ctx)
    }

    pub fn migrate_config(ctx: Context<MigrateConfig>) -> Result<()> {
        instructions::migrate_config::handle_migrate_config(ctx)
    }

    pub fn set_admin(ctx: Context<AdminOnly>, new_admin: Pubkey) -> Result<()> {
        instructions::admin::handle_set_admin(ctx, new_admin)
    }

    pub fn accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
        instructions::admin::handle_accept_admin(ctx)
    }

    pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
        instructions::admin::handle_set_paused(ctx, paused)
    }

    pub fn register_merchant(
        ctx: Context<RegisterMerchant>,
        id: u64,
        args: RegisterMerchantArgs,
    ) -> Result<()> {
        instructions::register_merchant::handle_register_merchant(ctx, id, args)
    }

    /// Delete a merchant (only when its point supply is zero).
    pub fn close_merchant(ctx: Context<CloseMerchant>) -> Result<()> {
        instructions::register_merchant::handle_close_merchant(ctx)
    }

    pub fn update_merchant(
        ctx: Context<UpdateMerchant>,
        base_earn_rate: Option<u64>,
    ) -> Result<()> {
        instructions::register_merchant::handle_update_merchant(ctx, base_earn_rate)
    }

    pub fn finalize_transfer_guard(ctx: Context<FinalizeTransferGuard>) -> Result<()> {
        instructions::finalize_transfer_guard::handle_finalize_transfer_guard(ctx)
    }

    /// Attach/update a custom metadata attribute on the point token (enrichment).
    pub fn set_token_attribute(
        ctx: Context<SetTokenAttribute>,
        key: String,
        value: String,
    ) -> Result<()> {
        instructions::set_token_attribute::handle_set_token_attribute(ctx, key, value)
    }

    /// Update a core token metadata field (0=name, 1=symbol, 2=uri).
    pub fn update_token_metadata(
        ctx: Context<SetTokenAttribute>,
        field_kind: u8,
        value: String,
    ) -> Result<()> {
        instructions::set_token_attribute::handle_update_token_metadata(ctx, field_kind, value)
    }

    /// Update the point token's interest-bearing (decay) rate.
    pub fn update_decay_rate(ctx: Context<SetTokenAttribute>, new_rate_bps: i16) -> Result<()> {
        instructions::set_token_attribute::handle_update_decay_rate(ctx, new_rate_bps)
    }

    pub fn earn_points(ctx: Context<EarnPoints>, amount_base: u64, visit_day: u32) -> Result<()> {
        instructions::earn_points::handle_earn_points(ctx, amount_base, visit_day)
    }

    /// Earn with a governed campaign applied (multiplier / flat bonus / quest).
    pub fn earn_points_campaign(
        ctx: Context<EarnPointsCampaign>,
        amount_base: u64,
        visit_day: u32,
    ) -> Result<()> {
        instructions::earn_points::handle_earn_points_campaign(ctx, amount_base, visit_day)
    }

    pub fn create_campaign(
        ctx: Context<CreateCampaign>,
        id: u64,
        args: CampaignArgs,
    ) -> Result<()> {
        instructions::campaigns::handle_create_campaign(ctx, id, args)
    }

    pub fn update_campaign(ctx: Context<UpdateCampaign>, args: UpdateCampaignArgs) -> Result<()> {
        instructions::campaigns::handle_update_campaign(ctx, args)
    }

    pub fn close_campaign(ctx: Context<CloseCampaign>) -> Result<()> {
        instructions::campaigns::handle_close_campaign(ctx)
    }

    // ── merchant enterprise controls ────────────────────────────────────────
    pub fn set_merchant_operator(
        ctx: Context<MerchantOwnerOnly>,
        operator: Pubkey,
        add: bool,
    ) -> Result<()> {
        instructions::merchant_admin::handle_set_merchant_operator(ctx, operator, add)
    }

    pub fn set_merchant_paused(ctx: Context<MerchantOwnerOnly>, paused: bool) -> Result<()> {
        instructions::merchant_admin::handle_set_merchant_paused(ctx, paused)
    }

    pub fn update_merchant_profile(
        ctx: Context<MerchantOwnerOnly>,
        category: u8,
        metadata_uri: String,
    ) -> Result<()> {
        instructions::merchant_admin::handle_update_merchant_profile(ctx, category, metadata_uri)
    }

    pub fn verify_merchant(ctx: Context<VerifyMerchant>, verified: bool) -> Result<()> {
        instructions::merchant_admin::handle_verify_merchant(ctx, verified)
    }

    // ── Accredited merchant identity (spec 11, phase 1) ──────────────────────

    /// Bind the merchant's authority-to-issue to an aegis accreditation root
    /// (owner-only). Configures the fall-to posture + grace window.
    pub fn set_merchant_trust(
        ctx: Context<SetMerchantTrust>,
        accreditation_root: Pubkey,
        subject_issuer: Pubkey,
        required_schema: u64,
        aegis_program: Pubkey,
        degrade_target: u8,
        grace_secs: i64,
    ) -> Result<()> {
        instructions::merchant_trust::handle_set_merchant_trust(
            ctx,
            accreditation_root,
            subject_issuer,
            required_schema,
            aegis_program,
            degrade_target,
            grace_secs,
        )
    }

    /// Permissionless crank: re-check the merchant's aegis accreditation and
    /// auto-degrade (after grace) or auto-restore its issuance posture.
    pub fn reverify_merchant(ctx: Context<ReverifyMerchant>) -> Result<()> {
        instructions::merchant_trust::handle_reverify_merchant(ctx)
    }

    /// Owner manual issuance-posture override — emergency freeze, or restore to
    /// NORMAL after resolving a dispute.
    pub fn set_merchant_issue_status(
        ctx: Context<SetMerchantIssueStatus>,
        status: u8,
    ) -> Result<()> {
        instructions::merchant_trust::handle_set_merchant_issue_status(ctx, status)
    }

    // ── Point-liability reserve (spec 11, phase 2) ───────────────────────────

    /// Open a stablecoin-backed liability reserve for the merchant (owner-only).
    pub fn open_reserve(
        ctx: Context<OpenReserve>,
        unit_value: u64,
        target_ratio_bps: u16,
    ) -> Result<()> {
        instructions::merchant_reserve::handle_open_reserve(ctx, unit_value, target_ratio_bps)
    }

    /// Deposit backing stablecoin into the reserve (owner-only).
    pub fn fund_reserve(ctx: Context<FundReserve>, amount: u64) -> Result<()> {
        instructions::merchant_reserve::handle_fund_reserve(ctx, amount)
    }

    /// Withdraw from the reserve — rejected if it would drop below the coverage
    /// required to back the point mint's current supply (owner-only).
    pub fn withdraw_reserve(ctx: Context<WithdrawReserve>, amount: u64) -> Result<()> {
        instructions::merchant_reserve::handle_withdraw_reserve(ctx, amount)
    }

    /// Emit a permissionless proof-of-reserves snapshot.
    pub fn attest_reserve(ctx: Context<AttestReserve>) -> Result<()> {
        instructions::merchant_reserve::handle_attest_reserve(ctx)
    }

    // ── Verified customer segmentation (spec 12, phase 1) ────────────────────

    /// Define the merchant's verified segments (owner-only). Each segment is an
    /// aegis `(issuer, schema)` predicate occupying one verdict-bitmap slot.
    pub fn set_merchant_segments(
        ctx: Context<SetMerchantSegments>,
        segments: Vec<Segment>,
    ) -> Result<()> {
        instructions::segmentation::handle_set_merchant_segments(ctx, segments)
    }

    /// Permissionless: refresh a customer's cached verdict for one segment by
    /// CPI-ing aegis `verify` off the hot path (spec 12 §4.1).
    pub fn refresh_customer_eligibility(
        ctx: Context<RefreshCustomerEligibility>,
        segment_index: u8,
    ) -> Result<()> {
        instructions::segmentation::handle_refresh_customer_eligibility(ctx, segment_index)
    }

    /// Anchor a period's economic-decision Merkle root on-chain (owner) —
    /// tamper-evident, provably complete (spec 13 §4.4).
    pub fn anchor_merchant_statement(
        ctx: Context<AnchorMerchantStatement>,
        period: u64,
        merkle_root: [u8; 32],
        decision_count: u64,
    ) -> Result<()> {
        instructions::merchant_statements::handle_anchor_merchant_statement(
            ctx,
            period,
            merkle_root,
            decision_count,
        )
    }

    /// Set the merchant's daily clawback cap (raw units; 0 = unlimited).
    pub fn set_clawback_cap(ctx: Context<MerchantOwnerOnly>, daily_cap_raw: u64) -> Result<()> {
        instructions::merchant_admin::handle_set_clawback_cap(ctx, daily_cap_raw)
    }

    /// Set the merchant's daily issuance cap in raw points (0 = unlimited) —
    /// the issuance-side blast-radius limiter (spec 13 §4.2).
    pub fn set_daily_issue_cap(ctx: Context<MerchantOwnerOnly>, daily_cap_raw: u64) -> Result<()> {
        instructions::merchant_admin::handle_set_daily_issue_cap(ctx, daily_cap_raw)
    }

    /// Adopt/update/disable scoped operator roles — separation of duties
    /// (spec 13 §4.1). Owner-only, opt-in.
    pub fn set_merchant_governance(
        ctx: Context<MerchantOwnerOnly>,
        enabled: bool,
        cashier: Pubkey,
        campaign_manager: Pubkey,
    ) -> Result<()> {
        instructions::merchant_admin::handle_set_merchant_governance(
            ctx,
            enabled,
            cashier,
            campaign_manager,
        )
    }

    pub fn create_achievement(
        ctx: Context<CreateAchievement>,
        id: u64,
        name: String,
        uri: String,
        threshold_lifetime: u64,
    ) -> Result<()> {
        instructions::achievements::handle_create_achievement(
            ctx,
            id,
            name,
            uri,
            threshold_lifetime,
        )
    }

    pub fn grant_achievement(ctx: Context<GrantAchievement>) -> Result<()> {
        instructions::achievements::handle_grant_achievement(ctx)
    }

    pub fn close_achievement(ctx: Context<CloseAchievement>) -> Result<()> {
        instructions::achievements::handle_close_achievement(ctx)
    }

    pub fn create_offer(
        ctx: Context<CreateOffer>,
        id: u64,
        price_points: u64,
        supply: u32,
    ) -> Result<()> {
        instructions::offers::handle_create_offer(ctx, id, price_points, supply)
    }

    pub fn close_offer(ctx: Context<CloseOffer>) -> Result<()> {
        instructions::offers::handle_close_offer(ctx)
    }

    pub fn redeem_offer(ctx: Context<RedeemOffer>, max_raw_amount: u64) -> Result<()> {
        instructions::offers::handle_redeem_offer(ctx, max_raw_amount)
    }

    pub fn create_alliance(ctx: Context<CreateAlliance>, id: u64, name: String) -> Result<()> {
        instructions::koinon::handle_create_alliance(ctx, id, name)
    }

    pub fn transfer_alliance_authority(
        ctx: Context<AllianceAuthorityOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        instructions::koinon::handle_transfer_alliance_authority(ctx, new_authority)
    }

    pub fn accept_alliance_authority(ctx: Context<AcceptAllianceAuthority>) -> Result<()> {
        instructions::koinon::handle_accept_alliance_authority(ctx)
    }

    pub fn set_alliance_paused(ctx: Context<AllianceAuthorityOnly>, paused: bool) -> Result<()> {
        instructions::koinon::handle_set_alliance_paused(ctx, paused)
    }

    pub fn set_alliance_params(
        ctx: Context<AllianceAuthorityOnly>,
        fee_bps: u16,
        min_rate_bps: u32,
        max_rate_bps: u32,
    ) -> Result<()> {
        instructions::koinon::handle_set_alliance_params(ctx, fee_bps, min_rate_bps, max_rate_bps)
    }

    pub fn update_alliance_profile(
        ctx: Context<AllianceAuthorityOnly>,
        category: u8,
        metadata_uri: String,
    ) -> Result<()> {
        instructions::koinon::handle_update_alliance_profile(ctx, category, metadata_uri)
    }

    /// Alliance authority suspends / reactivates a member.
    pub fn set_member_active(ctx: Context<SetMemberActive>, active: bool) -> Result<()> {
        instructions::koinon::handle_set_member_active(ctx, active)
    }

    pub fn join_alliance(
        ctx: Context<JoinAlliance>,
        rate_bps_to_alliance: u32,
        swap_in_budget_raw: u64,
    ) -> Result<()> {
        instructions::koinon::handle_join_alliance(ctx, rate_bps_to_alliance, swap_in_budget_raw)
    }

    pub fn leave_alliance(ctx: Context<LeaveAlliance>) -> Result<()> {
        instructions::koinon::handle_leave_alliance(ctx)
    }

    pub fn set_swap_rate(ctx: Context<SetSwapRate>, new_rate: u32) -> Result<()> {
        instructions::koinon::handle_set_swap_rate(ctx, new_rate)
    }

    pub fn set_swap_budget(ctx: Context<SetSwapBudget>, new_budget: u64) -> Result<()> {
        instructions::koinon::handle_set_swap_budget(ctx, new_budget)
    }

    pub fn swap_points(
        ctx: Context<SwapPoints>,
        ui_amount: u64,
        max_raw_in: u64,
        min_raw_out: u64,
    ) -> Result<()> {
        instructions::koinon::handle_swap_points(ctx, ui_amount, max_raw_in, min_raw_out)
    }

    pub fn clawback<'info>(
        ctx: Context<'info, ClawbackPoints<'info>>,
        amount_raw: u64,
        reason_code: u16,
    ) -> Result<()> {
        instructions::clawback::handle_clawback(ctx, amount_raw, reason_code)
    }

    pub fn close_receipt(ctx: Context<CloseReceipt>) -> Result<()> {
        instructions::offers::handle_close_receipt(ctx)
    }
}
