pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("Am2X4B1SCnJKXL8Yir2j6yGpHAKrmwcf2E5aKnA9BZV");

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

    pub fn close_receipt(ctx: Context<CloseReceipt>) -> Result<()> {
        instructions::offers::handle_close_receipt(ctx)
    }
}
