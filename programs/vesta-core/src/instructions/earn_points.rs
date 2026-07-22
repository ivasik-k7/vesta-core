use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_2022::{mint_to, MintTo, Token2022},
    token_interface::{Mint, TokenAccount},
};

use crate::{
    constants::{
        CAMPAIGN_PROGRESS_SEED, CONFIG_SEED, CUSTOMER_SEED, MAX_EARN_PER_TX,
        MAX_TOTAL_MULTIPLIER_BPS, MERCHANT_SEED, SECONDS_PER_DAY, STREAK_BPS_PER_DAY,
        STREAK_DAYS_CAP, TIER_THRESHOLDS,
    },
    error::VestaError,
    events::{CampaignBonusPaid, PointsEarned},
    state::{campaign_kind, Campaign, CampaignProgress, Config, CustomerProfile, Merchant},
};

/// Shared accrual core: rolls the streak, computes the streak-boosted base plus
/// a pre-computed campaign `bonus_raw`, applies the per-tx cap, and updates
/// customer/merchant stats + tier. Returns `(minted, streak_days, total_bps)`.
/// The caller performs the mint CPI with the returned amount.
fn accrue(
    profile: &mut CustomerProfile,
    merchant: &mut Merchant,
    amount_base: u64,
    unix_day: u32,
    bonus_raw: u64,
) -> Result<(u64, u16, u64)> {
    // Streak: +1 on consecutive days, hold on same-day repeats, reset otherwise.
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
    let total_bps = 10_000u64
        .checked_add(streak_bps)
        .ok_or(VestaError::MultiplierOverflow)?
        .min(MAX_TOTAL_MULTIPLIER_BPS);

    let base_minted = u128::from(amount_base)
        .checked_mul(u128::from(merchant.base_earn_rate))
        .and_then(|v| v.checked_mul(u128::from(total_bps)))
        .and_then(|v| v.checked_div(10_000))
        .ok_or(VestaError::MultiplierOverflow)?;
    let minted = u64::try_from(base_minted)
        .ok()
        .and_then(|b| b.checked_add(bonus_raw))
        .ok_or(VestaError::MultiplierOverflow)?;
    require!(minted <= MAX_EARN_PER_TX, VestaError::EarnCapExceeded);

    profile.lifetime_earned = profile
        .lifetime_earned
        .checked_add(minted)
        .ok_or(VestaError::Overflow)?;
    profile.lifetime_spend_base = profile.lifetime_spend_base.saturating_add(amount_base);
    merchant.lifetime_points_issued = merchant
        .lifetime_points_issued
        .checked_add(u128::from(minted))
        .ok_or(VestaError::Overflow)?;

    // Issuance circuit breaker (spec 13 §4.2): bound raw points minted per UTC
    // day. `0` = unlimited (default), so a merchant that never sets a cap is
    // unaffected. Symmetric to the clawback daily cap.
    if merchant.issue_day != unix_day {
        merchant.issue_day = unix_day;
        merchant.issued_today = 0;
    }
    merchant.issued_today = merchant
        .issued_today
        .checked_add(minted)
        .ok_or(VestaError::Overflow)?;
    if merchant.daily_issue_cap_raw > 0 {
        require!(
            merchant.issued_today <= merchant.daily_issue_cap_raw,
            VestaError::DailyIssuanceCapExceeded
        );
    }
    profile.tier = u8::try_from(
        TIER_THRESHOLDS
            .iter()
            .filter(|&&t| profile.lifetime_earned >= t)
            .count()
            .saturating_sub(1),
    )
    .unwrap_or(u8::MAX);

    Ok((minted, profile.streak_days, total_bps))
}

/// The streak-boosted marginal bps a MULTIPLIER campaign may add, honoring the
/// joint cap over streak + campaign composition.
fn campaign_headroom_bps(total_bps: u64) -> u64 {
    MAX_TOTAL_MULTIPLIER_BPS.saturating_sub(total_bps)
}

// ── plain earn (streak only) ─────────────────────────────────────────────────

#[derive(Accounts)]
pub struct EarnPoints<'info> {
    #[account(mut)]
    pub merchant_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant.authority.as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Account<'info, Merchant>,

    /// CHECK: identity only — the profile and ATA PDAs are derived from this key.
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
    require!(!ctx.accounts.merchant.paused, VestaError::MerchantPaused);
    // Accreditation gate (spec 11 §4.1): a degraded issuance posture freezes
    // earn. Default NORMAL, so a merchant that never adopts accreditation is
    // unaffected. Redemption and clawback deliberately do NOT check this.
    require!(
        ctx.accounts.merchant.issue_status == crate::constants::issue_status::NORMAL,
        VestaError::IssuanceFrozen
    );
    require!(
        ctx.accounts
            .merchant
            .may_earn(&ctx.accounts.merchant_authority.key()),
        VestaError::Unauthorized
    );
    require!(amount_base > 0, VestaError::InvalidAmount);

    let unix_day = current_day(&visit_day)?;
    init_profile_identity(
        &mut ctx.accounts.customer_profile,
        &mut ctx.accounts.merchant,
        ctx.accounts.customer.key(),
        ctx.bumps.customer_profile,
    )?;

    let (minted, streak_days, total_bps) = accrue(
        &mut ctx.accounts.customer_profile,
        &mut ctx.accounts.merchant,
        amount_base,
        unix_day,
        0,
    )?;
    mint_points(
        &ctx.accounts.token_program,
        &ctx.accounts.point_mint,
        &ctx.accounts.customer_ata,
        &ctx.accounts.merchant,
        minted,
    )?;

    emit!(PointsEarned {
        merchant: ctx.accounts.merchant.key(),
        customer: ctx.accounts.customer.key(),
        base: amount_base,
        minted,
        multiplier_bps: total_bps,
        streak_days,
    });
    Ok(())
}

// ── governed campaign earn (multiplier / flat bonus / quest) ─────────────────

#[derive(Accounts)]
pub struct EarnPointsCampaign<'info> {
    #[account(mut)]
    pub merchant_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant.authority.as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Box<Account<'info, Merchant>>,

    /// CHECK: identity only.
    pub customer: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = merchant_authority,
        space = 8 + CustomerProfile::INIT_SPACE,
        seeds = [CUSTOMER_SEED, merchant.key().as_ref(), customer.key().as_ref()],
        bump,
    )]
    pub customer_profile: Box<Account<'info, CustomerProfile>>,

    #[account(
        mut,
        has_one = merchant @ VestaError::MerchantMismatch,
    )]
    pub campaign: Box<Account<'info, Campaign>>,

    #[account(
        init_if_needed,
        payer = merchant_authority,
        space = 8 + CampaignProgress::INIT_SPACE,
        seeds = [CAMPAIGN_PROGRESS_SEED, campaign.key().as_ref(), customer.key().as_ref()],
        bump,
    )]
    pub campaign_progress: Box<Account<'info, CampaignProgress>>,

    #[account(
        mut,
        seeds = [crate::constants::MINT_SEED, merchant.key().as_ref()],
        bump = merchant.mint_bump,
    )]
    pub point_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init_if_needed,
        payer = merchant_authority,
        associated_token::mint = point_mint,
        associated_token::authority = customer,
        associated_token::token_program = token_program,
    )]
    pub customer_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,

    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_earn_points_campaign(
    ctx: Context<EarnPointsCampaign>,
    amount_base: u64,
    visit_day: u32,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(!ctx.accounts.merchant.paused, VestaError::MerchantPaused);
    // Accreditation gate (spec 11 §4.1): a degraded issuance posture freezes
    // earn. Default NORMAL, so a merchant that never adopts accreditation is
    // unaffected. Redemption and clawback deliberately do NOT check this.
    require!(
        ctx.accounts.merchant.issue_status == crate::constants::issue_status::NORMAL,
        VestaError::IssuanceFrozen
    );
    require!(
        ctx.accounts
            .merchant
            .may_earn(&ctx.accounts.merchant_authority.key()),
        VestaError::Unauthorized
    );
    require!(amount_base > 0, VestaError::InvalidAmount);

    let now = Clock::get()?.unix_timestamp;
    let unix_day = current_day(&visit_day)?;
    init_profile_identity(
        &mut ctx.accounts.customer_profile,
        &mut ctx.accounts.merchant,
        ctx.accounts.customer.key(),
        ctx.bumps.customer_profile,
    )?;

    // Campaign eligibility.
    let campaign = &ctx.accounts.campaign;
    require!(campaign.is_live(now), VestaError::CampaignInactive);
    require!(
        amount_base >= campaign.min_spend_base,
        VestaError::CampaignNotEligible
    );
    require!(
        ctx.accounts.customer_profile.tier >= campaign.min_tier,
        VestaError::CampaignNotEligible
    );

    // Fresh progress → count a participant. Also treat a progress account that
    // survived a close+recreate of this campaign id as fresh: the campaign PDA
    // is identical across instances, so we key freshness on the creation slot
    // and reset the stale quest/bonus state (AUDIT M-3).
    let progress = &mut ctx.accounts.campaign_progress;
    let fresh_progress =
        progress.campaign == Pubkey::default() || progress.campaign_slot != campaign.created_slot;
    if fresh_progress {
        progress.campaign = campaign.key();
        progress.customer = ctx.accounts.customer.key();
        progress.campaign_slot = campaign.created_slot;
        progress.visits = 0;
        progress.bonus_drawn = 0;
        progress.completed = false;
        progress.bump = ctx.bumps.campaign_progress;
    }

    // Streak-only base to know multiplier headroom, before adding the bonus.
    // (accrue with bonus 0 first would double-mint; instead compute base bps here.)
    let streak_preview = preview_total_bps(&ctx.accounts.customer_profile, unix_day)?;

    // Gross bonus by kind.
    let mut quest_completed = false;
    let gross_bonus: u64 = match campaign.kind {
        campaign_kind::MULTIPLIER => {
            let eff_bps =
                u64::from(campaign.multiplier_bps).min(campaign_headroom_bps(streak_preview));
            u128::from(amount_base)
                .checked_mul(u128::from(ctx.accounts.merchant.base_earn_rate))
                .and_then(|v| v.checked_mul(u128::from(eff_bps)))
                .and_then(|v| v.checked_div(10_000))
                .and_then(|v| u64::try_from(v).ok())
                .ok_or(VestaError::MultiplierOverflow)?
        }
        campaign_kind::FLAT_BONUS => campaign.flat_bonus,
        campaign_kind::QUEST => {
            progress.visits = progress.visits.saturating_add(1);
            // Tentative — the quest only *completes* if the full reward is
            // actually paid after the clamps below; otherwise it stays open.
            if !progress.completed && progress.visits >= campaign.quest_target {
                campaign.quest_reward
            } else {
                0
            }
        }
        _ => 0,
    };

    // Clamp by per-customer cap, then by remaining budget.
    let mut bonus = gross_bonus;
    if campaign.per_customer_cap > 0 {
        let room = campaign
            .per_customer_cap
            .saturating_sub(progress.bonus_drawn);
        bonus = bonus.min(room);
    }
    if campaign.points_budget > 0 {
        let room = campaign.points_budget.saturating_sub(campaign.points_spent);
        bonus = bonus.min(room);
    }

    // A quest completes only when its full reward clears the caps — a clamped
    // payout leaves it open to retry rather than burning the completion.
    if campaign.kind == campaign_kind::QUEST && gross_bonus > 0 && bonus == gross_bonus {
        quest_completed = true;
    }

    let (minted, streak_days, total_bps) = accrue(
        &mut ctx.accounts.customer_profile,
        &mut ctx.accounts.merchant,
        amount_base,
        unix_day,
        bonus,
    )?;
    mint_points(
        &ctx.accounts.token_program,
        &ctx.accounts.point_mint,
        &ctx.accounts.customer_ata,
        &ctx.accounts.merchant,
        minted,
    )?;

    // Campaign bookkeeping.
    let campaign = &mut ctx.accounts.campaign;
    if fresh_progress {
        campaign.participant_count = campaign.participant_count.saturating_add(1);
    }
    campaign.points_spent = campaign.points_spent.saturating_add(bonus);
    campaign.redemptions = campaign.redemptions.saturating_add(1);
    let progress = &mut ctx.accounts.campaign_progress;
    progress.bonus_drawn = progress.bonus_drawn.saturating_add(bonus);
    if quest_completed {
        progress.completed = true;
        ctx.accounts.customer_profile.campaigns_completed = ctx
            .accounts
            .customer_profile
            .campaigns_completed
            .saturating_add(1);
    }

    emit!(PointsEarned {
        merchant: ctx.accounts.merchant.key(),
        customer: ctx.accounts.customer.key(),
        base: amount_base,
        minted,
        multiplier_bps: total_bps,
        streak_days,
    });
    emit!(CampaignBonusPaid {
        merchant: ctx.accounts.merchant.key(),
        campaign: ctx.accounts.campaign.id,
        customer: ctx.accounts.customer.key(),
        kind: ctx.accounts.campaign.kind,
        bonus,
        quest_completed,
    });
    Ok(())
}

// ── shared helpers ───────────────────────────────────────────────────────────

fn current_day(visit_day: &u32) -> Result<u32> {
    let unix_day = u32::try_from(Clock::get()?.unix_timestamp / SECONDS_PER_DAY)
        .map_err(|_| VestaError::Overflow)?;
    require!(*visit_day == unix_day, VestaError::StaleVisitDay);
    Ok(unix_day)
}

fn init_profile_identity(
    profile: &mut CustomerProfile,
    merchant: &mut Account<Merchant>,
    customer: Pubkey,
    bump: u8,
) -> Result<()> {
    if profile.wallet == Pubkey::default() {
        profile.wallet = customer;
        profile.merchant = merchant.key();
        profile.bump = bump;
        merchant.customer_count = merchant
            .customer_count
            .checked_add(1)
            .ok_or(VestaError::Overflow)?;
    }
    Ok(())
}

/// The streak bps this earn WILL use (without mutating state) — for headroom.
fn preview_total_bps(profile: &CustomerProfile, unix_day: u32) -> Result<u64> {
    let next_streak = if profile.last_visit_day == unix_day {
        profile.streak_days.max(1)
    } else if profile.last_visit_day.checked_add(1) == Some(unix_day) {
        profile.streak_days.saturating_add(1)
    } else {
        1
    };
    let streak_bps = u64::from(next_streak.min(STREAK_DAYS_CAP))
        .checked_mul(u64::from(STREAK_BPS_PER_DAY))
        .ok_or(VestaError::MultiplierOverflow)?;
    Ok(10_000u64
        .checked_add(streak_bps)
        .ok_or(VestaError::MultiplierOverflow)?
        .min(MAX_TOTAL_MULTIPLIER_BPS))
}

fn mint_points<'info>(
    token_program: &Program<'info, Token2022>,
    point_mint: &InterfaceAccount<'info, Mint>,
    customer_ata: &InterfaceAccount<'info, TokenAccount>,
    merchant: &Account<'info, Merchant>,
    amount: u64,
) -> Result<()> {
    let authority_key = merchant.authority;
    let id_bytes = merchant.id.to_le_bytes();
    let merchant_seeds: &[&[u8]] = &[
        MERCHANT_SEED,
        authority_key.as_ref(),
        &id_bytes,
        &[merchant.bump],
    ];
    mint_to(
        CpiContext::new_with_signer(
            token_program.key(),
            MintTo {
                mint: point_mint.to_account_info(),
                to: customer_ata.to_account_info(),
                authority: merchant.to_account_info(),
            },
            &[merchant_seeds],
        ),
        amount,
    )
}
