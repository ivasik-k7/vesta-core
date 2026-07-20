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
    name: "VESTA — vesta_core (living loyalty protocol)",
    project_url: "https://github.com/ivasik-k7/vesta-core",
    contacts: "email:kovtun.ivan@proton.meink:https://github.com/ivasik-k7/vesta-core/blob/main/SECURITY.md",
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
        args: RegisterMerchantArgs,
    ) -> Result<()> {
        instructions::register_merchant::handle_register_merchant(ctx, args)
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

    pub fn earn_points(ctx: Context<EarnPoints>, amount_base: u64, visit_day: u32) -> Result<()> {
        instructions::earn_points::handle_earn_points(ctx, amount_base, visit_day)
    }

    pub fn create_campaign(
        ctx: Context<CreateCampaign>,
        id: u64,
        multiplier_bps: u16,
        starts_at: i64,
        ends_at: i64,
    ) -> Result<()> {
        instructions::campaigns::handle_create_campaign(ctx, id, multiplier_bps, starts_at, ends_at)
    }

    pub fn close_campaign(ctx: Context<CloseCampaign>) -> Result<()> {
        instructions::campaigns::handle_close_campaign(ctx)
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
