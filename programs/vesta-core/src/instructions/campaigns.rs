use anchor_lang::prelude::*;

use crate::{
    constants::{CAMPAIGN_MAX_BPS, CAMPAIGN_SEED, CONFIG_SEED, MERCHANT_SEED},
    error::VestaError,
    events::{CampaignClosed, CampaignCreated},
    state::{Campaign, Config, Merchant},
};

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct CreateCampaign<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        init,
        payer = authority,
        space = 8 + Campaign::INIT_SPACE,
        seeds = [CAMPAIGN_SEED, merchant.key().as_ref(), &id.to_le_bytes()],
        bump,
    )]
    pub campaign: Account<'info, Campaign>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    pub system_program: Program<'info, System>,
}

pub fn handle_create_campaign(
    ctx: Context<CreateCampaign>,
    id: u64,
    multiplier_bps: u16,
    starts_at: i64,
    ends_at: i64,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(
        multiplier_bps > 0 && multiplier_bps <= CAMPAIGN_MAX_BPS,
        VestaError::CampaignWindowInvalid
    );
    require!(starts_at < ends_at, VestaError::CampaignWindowInvalid);

    let campaign = &mut ctx.accounts.campaign;
    campaign.merchant = ctx.accounts.merchant.key();
    campaign.id = id;
    campaign.multiplier_bps = multiplier_bps;
    campaign.starts_at = starts_at;
    campaign.ends_at = ends_at;
    campaign.active = true;
    campaign.bump = ctx.bumps.campaign;

    emit!(CampaignCreated {
        merchant: campaign.merchant,
        id,
        multiplier_bps,
        starts_at,
        ends_at,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct CloseCampaign<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        mut,
        close = authority,
        has_one = merchant @ VestaError::MerchantMismatch,
    )]
    pub campaign: Account<'info, Campaign>,
}

pub fn handle_close_campaign(ctx: Context<CloseCampaign>) -> Result<()> {
    emit!(CampaignClosed {
        merchant: ctx.accounts.merchant.key(),
        id: ctx.accounts.campaign.id,
    });
    Ok(())
}
