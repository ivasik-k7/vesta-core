use anchor_lang::prelude::*;

use crate::{
    constants::{
        CAMPAIGN_MAX_BPS, CAMPAIGN_SEED, CONFIG_SEED, MAX_CAMPAIGN_BONUS, MAX_CAMPAIGN_NAME_LEN,
        MAX_QUEST_TARGET, MERCHANT_SEED,
    },
    error::VestaError,
    events::{CampaignClosed, CampaignCreated, CampaignUpdated},
    state::{campaign_kind, Campaign, Config, Merchant},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct CampaignArgs {
    pub kind: u8,
    pub multiplier_bps: u16,
    pub flat_bonus: u64,
    pub quest_target: u16,
    pub quest_reward: u64,
    pub min_spend_base: u64,
    pub min_tier: u8,
    pub points_budget: u64,
    pub per_customer_cap: u64,
    pub starts_at: i64,
    pub ends_at: i64,
    pub name: String,
}

fn validate(args: &CampaignArgs) -> Result<()> {
    require!(
        args.name.len() <= MAX_CAMPAIGN_NAME_LEN,
        VestaError::StringTooLong
    );
    require!(
        args.starts_at < args.ends_at,
        VestaError::CampaignWindowInvalid
    );
    match args.kind {
        campaign_kind::MULTIPLIER => require!(
            args.multiplier_bps > 0 && args.multiplier_bps <= CAMPAIGN_MAX_BPS,
            VestaError::CampaignWindowInvalid
        ),
        campaign_kind::FLAT_BONUS => require!(
            args.flat_bonus > 0 && args.flat_bonus <= MAX_CAMPAIGN_BONUS,
            VestaError::CampaignWindowInvalid
        ),
        campaign_kind::QUEST => {
            require!(
                args.quest_target > 0 && args.quest_target <= MAX_QUEST_TARGET,
                VestaError::CampaignWindowInvalid
            );
            require!(
                args.quest_reward > 0 && args.quest_reward <= MAX_CAMPAIGN_BONUS,
                VestaError::CampaignWindowInvalid
            );
            // A cap or budget below the reward clamps every payout, so the quest
            // can never register as complete. Reject the impossible config
            // (0 = unlimited on both, which is fine) (AUDIT L-5).
            require!(
                args.per_customer_cap == 0 || args.per_customer_cap >= args.quest_reward,
                VestaError::CampaignWindowInvalid
            );
            require!(
                args.points_budget == 0 || args.points_budget >= args.quest_reward,
                VestaError::CampaignWindowInvalid
            );
        }
        _ => return err!(VestaError::CampaignWindowInvalid),
    }
    Ok(())
}

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct CreateCampaign<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, merchant.authority.as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
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
    args: CampaignArgs,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(!ctx.accounts.merchant.paused, VestaError::MerchantPaused);
    require!(
        ctx.accounts
            .merchant
            .can_operate(&ctx.accounts.authority.key()),
        VestaError::Unauthorized
    );
    validate(&args)?;

    let c = &mut ctx.accounts.campaign;
    c.merchant = ctx.accounts.merchant.key();
    c.id = id;
    c.kind = args.kind;
    c.multiplier_bps = args.multiplier_bps;
    c.flat_bonus = args.flat_bonus;
    c.quest_target = args.quest_target;
    c.quest_reward = args.quest_reward;
    c.min_spend_base = args.min_spend_base;
    c.min_tier = args.min_tier;
    c.points_budget = args.points_budget;
    c.points_spent = 0;
    c.per_customer_cap = args.per_customer_cap;
    c.starts_at = args.starts_at;
    c.ends_at = args.ends_at;
    c.participant_count = 0;
    c.redemptions = 0;
    c.name = args.name;
    c.active = true;
    c.paused = false;
    c.created_slot = Clock::get()?.slot;
    c.bump = ctx.bumps.campaign;

    emit!(CampaignCreated {
        merchant: c.merchant,
        id,
        kind: c.kind,
        multiplier_bps: c.multiplier_bps,
        starts_at: c.starts_at,
        ends_at: c.ends_at,
    });
    Ok(())
}

/// Partial in-flight retune. Owner may extend the window, grow the budget,
/// adjust the per-customer cap, or pause/resume — not change the kind or its
/// core payout params (those define the offer customers signed up for).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, Default)]
pub struct UpdateCampaignArgs {
    pub ends_at: Option<i64>,
    pub points_budget: Option<u64>,
    pub per_customer_cap: Option<u64>,
    pub paused: Option<bool>,
}

#[derive(Accounts)]
pub struct UpdateCampaign<'info> {
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(mut, has_one = merchant @ VestaError::MerchantMismatch)]
    pub campaign: Account<'info, Campaign>,
}

pub fn handle_update_campaign(
    ctx: Context<UpdateCampaign>,
    args: UpdateCampaignArgs,
) -> Result<()> {
    let c = &mut ctx.accounts.campaign;
    if let Some(ends_at) = args.ends_at {
        require!(ends_at > c.starts_at, VestaError::CampaignWindowInvalid);
        c.ends_at = ends_at;
    }
    let is_quest = c.kind == campaign_kind::QUEST;
    if let Some(budget) = args.points_budget {
        // Never below what has already been paid out.
        require!(
            budget == 0 || budget >= c.points_spent,
            VestaError::CampaignWindowInvalid
        );
        // A quest budget below the reward makes completion unreachable (L-5).
        require!(
            !is_quest || budget == 0 || budget >= c.quest_reward,
            VestaError::CampaignWindowInvalid
        );
        c.points_budget = budget;
    }
    if let Some(cap) = args.per_customer_cap {
        require!(
            !is_quest || cap == 0 || cap >= c.quest_reward,
            VestaError::CampaignWindowInvalid
        );
        c.per_customer_cap = cap;
    }
    if let Some(paused) = args.paused {
        c.paused = paused;
    }

    emit!(CampaignUpdated {
        merchant: c.merchant,
        id: c.id,
        paused: c.paused,
        points_budget: c.points_budget,
        ends_at: c.ends_at,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct CloseCampaign<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
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
