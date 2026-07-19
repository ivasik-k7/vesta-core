use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_2022::{mint_to, MintTo, Token2022},
    token_interface::{Mint, TokenAccount},
};

use crate::{
    constants::{
        CONFIG_SEED, CUSTOMER_SEED, MAX_EARN_PER_TX, MAX_TOTAL_MULTIPLIER_BPS, MERCHANT_SEED,
        SECONDS_PER_DAY, STREAK_BPS_PER_DAY, STREAK_DAYS_CAP, TIER_THRESHOLDS,
    },
    error::VestaError,
    events::PointsEarned,
    state::{Campaign, Config, CustomerProfile, Merchant},
};

#[derive(Accounts)]
pub struct EarnPoints<'info> {
    #[account(mut)]
    pub merchant_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant_authority.key().as_ref()],
        bump = merchant.bump,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Account<'info, Merchant>,

    /// CHECK: identity only — receives no privileges; the profile and ATA PDAs
    /// are derived from this key.
    pub customer: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = merchant_authority,
        space = 8 + CustomerProfile::INIT_SPACE,
        seeds = [CUSTOMER_SEED, merchant.key().as_ref(), customer.key().as_ref()],
        bump,
    )]
    pub customer_profile: Account<'info, CustomerProfile>,

    #[account(
        mut,
        seeds = [crate::constants::MINT_SEED, merchant.key().as_ref()],
        bump = merchant.mint_bump,
    )]
    pub point_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init_if_needed,
        payer = merchant_authority,
        associated_token::mint = point_mint,
        associated_token::authority = customer,
        associated_token::token_program = token_program,
    )]
    pub customer_ata: InterfaceAccount<'info, TokenAccount>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    /// Optional earn-multiplier campaign; the program applies *the supplied*
    /// campaign — it cannot claim "best" on-chain. Validated in the handler.
    pub campaign: Option<Account<'info, Campaign>>,

    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_earn_points(
    ctx: Context<EarnPoints>,
    amount_base: u64,
    visit_day: u32,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(amount_base > 0, VestaError::InvalidAmount);

    let clock = Clock::get()?;
    let unix_day =
        u32::try_from(clock.unix_timestamp / SECONDS_PER_DAY).map_err(|_| VestaError::Overflow)?;
    require!(visit_day == unix_day, VestaError::StaleVisitDay);

    let profile = &mut ctx.accounts.customer_profile;
    let merchant = &mut ctx.accounts.merchant;

    // Fresh profile: wire identity fields and count the customer once.
    if profile.wallet == Pubkey::default() {
        profile.wallet = ctx.accounts.customer.key();
        profile.merchant = merchant.key();
        profile.bump = ctx.bumps.customer_profile;
        merchant.customer_count = merchant
            .customer_count
            .checked_add(1)
            .ok_or(VestaError::Overflow)?;
    }

    // Streak: +1 on consecutive days, keep on same-day repeats, reset otherwise.
    profile.streak_days = if profile.last_visit_day == unix_day {
        profile.streak_days.max(1)
    } else if profile.last_visit_day.checked_add(1) == Some(unix_day) {
        profile.streak_days.saturating_add(1)
    } else {
        1
    };
    profile.last_visit_day = unix_day;

    let streak_bps = u64::from(profile.streak_days.min(STREAK_DAYS_CAP))
        .checked_mul(u64::from(STREAK_BPS_PER_DAY))
        .ok_or(VestaError::MultiplierOverflow)?;
    let campaign_bps = match ctx.accounts.campaign.as_ref() {
        Some(campaign) => {
            require_keys_eq!(
                campaign.merchant,
                merchant.key(),
                VestaError::MerchantMismatch
            );
            require!(campaign.active, VestaError::CampaignInactive);
            let now = clock.unix_timestamp;
            require!(
                campaign.starts_at <= now && now < campaign.ends_at,
                VestaError::CampaignInactive
            );
            u64::from(campaign.multiplier_bps)
        }
        None => 0,
    };
    let total_bps = 10_000u64
        .checked_add(streak_bps)
        .and_then(|v| v.checked_add(campaign_bps))
        .ok_or(VestaError::MultiplierOverflow)?
        .min(MAX_TOTAL_MULTIPLIER_BPS);

    let minted_wide = u128::from(amount_base)
        .checked_mul(u128::from(merchant.base_earn_rate))
        .and_then(|v| v.checked_mul(u128::from(total_bps)))
        .and_then(|v| v.checked_div(10_000))
        .ok_or(VestaError::MultiplierOverflow)?;
    let minted = u64::try_from(minted_wide).map_err(|_| VestaError::MultiplierOverflow)?;
    require!(minted <= MAX_EARN_PER_TX, VestaError::EarnCapExceeded);

    let authority_key = merchant.authority;
    let merchant_seeds: &[&[u8]] = &[MERCHANT_SEED, authority_key.as_ref(), &[merchant.bump]];
    mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            MintTo {
                mint: ctx.accounts.point_mint.to_account_info(),
                to: ctx.accounts.customer_ata.to_account_info(),
                authority: merchant.to_account_info(),
            },
            &[merchant_seeds],
        ),
        minted,
    )?;

    profile.lifetime_earned = profile
        .lifetime_earned
        .checked_add(minted)
        .ok_or(VestaError::Overflow)?;
    merchant.lifetime_points_issued = merchant
        .lifetime_points_issued
        .checked_add(u128::from(minted))
        .ok_or(VestaError::Overflow)?;

    profile.tier = u8::try_from(
        TIER_THRESHOLDS
            .iter()
            .filter(|&&t| profile.lifetime_earned >= t)
            .count()
            .saturating_sub(1),
    )
    .unwrap_or(u8::MAX);

    emit!(PointsEarned {
        merchant: merchant.key(),
        customer: ctx.accounts.customer.key(),
        base: amount_base,
        minted,
        multiplier_bps: total_bps,
        streak_days: profile.streak_days,
    });
    Ok(())
}
