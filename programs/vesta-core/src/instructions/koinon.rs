use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_2022::{burn, mint_to, Burn, MintTo, Token2022},
    token_interface::{Mint, TokenAccount},
};

use crate::{
    constants::{
        ALLIANCE_SEED, CONFIG_SEED, MAX_ALLIANCE_FEE_BPS, MAX_METADATA_URI_LEN, MAX_NAME_LEN,
        MEMBER_SEED, MERCHANT_SEED, MINT_SEED, SECONDS_PER_DAY,
    },
    error::VestaError,
    events::{
        AllianceAuthorityChanged, AllianceAuthorityProposed, AllianceCreated, AllianceJoined,
        AllianceLeft, AllianceParamsSet, AlliancePausedSet, MemberActiveSet, PointsSwapped,
        SwapBudgetSet, SwapRateSet,
    },
    state::{Alliance, AllianceMember, Config, Merchant},
};

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct CreateAlliance<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,

    #[account(
        init,
        payer = creator,
        space = 8 + Alliance::INIT_SPACE,
        seeds = [ALLIANCE_SEED, creator.key().as_ref(), &id.to_le_bytes()],
        bump,
    )]
    pub alliance: Account<'info, Alliance>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    pub system_program: Program<'info, System>,
}

pub fn handle_create_alliance(ctx: Context<CreateAlliance>, id: u64, name: String) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(name.len() <= MAX_NAME_LEN, VestaError::StringTooLong);

    let alliance = &mut ctx.accounts.alliance;
    alliance.id = id;
    alliance.authority = ctx.accounts.creator.key();
    alliance.pending_authority = None;
    alliance.name = name;
    alliance.member_count = 0;
    alliance.paused = false;
    alliance.fee_bps = 0;
    alliance.min_rate_bps = 0;
    alliance.max_rate_bps = 0;
    alliance.category = 0;
    alliance.metadata_uri = String::new();
    alliance.total_swaps = 0;
    alliance.total_ui_volume = 0;
    alliance.bump = ctx.bumps.alliance;

    emit!(AllianceCreated {
        alliance: alliance.key(),
        id,
        authority: alliance.authority,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct AllianceAuthorityOnly<'info> {
    pub authority: Signer<'info>,

    #[account(mut, has_one = authority @ VestaError::Unauthorized)]
    pub alliance: Account<'info, Alliance>,
}

pub fn handle_transfer_alliance_authority(
    ctx: Context<AllianceAuthorityOnly>,
    new_authority: Pubkey,
) -> Result<()> {
    let alliance = &mut ctx.accounts.alliance;
    alliance.pending_authority = Some(new_authority);

    emit!(AllianceAuthorityProposed {
        alliance: alliance.key(),
        old: alliance.authority,
        new: new_authority,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct AcceptAllianceAuthority<'info> {
    pub pending_authority: Signer<'info>,

    #[account(mut)]
    pub alliance: Account<'info, Alliance>,
}

pub fn handle_accept_alliance_authority(ctx: Context<AcceptAllianceAuthority>) -> Result<()> {
    let alliance = &mut ctx.accounts.alliance;
    require!(
        alliance.pending_authority == Some(ctx.accounts.pending_authority.key()),
        VestaError::PendingAdminMismatch
    );

    let old = alliance.authority;
    alliance.authority = ctx.accounts.pending_authority.key();
    alliance.pending_authority = None;

    emit!(AllianceAuthorityChanged {
        alliance: alliance.key(),
        old,
        new: alliance.authority,
    });
    Ok(())
}

/// Enforce the alliance's member-rate governance bounds (0 = unbounded).
fn check_rate_bounds(alliance: &Alliance, rate: u32) -> Result<()> {
    if alliance.min_rate_bps > 0 {
        require!(rate >= alliance.min_rate_bps, VestaError::SwapRateOutOfBounds);
    }
    if alliance.max_rate_bps > 0 {
        require!(rate <= alliance.max_rate_bps, VestaError::SwapRateOutOfBounds);
    }
    Ok(())
}

pub fn handle_set_alliance_paused(ctx: Context<AllianceAuthorityOnly>, paused: bool) -> Result<()> {
    let a = &mut ctx.accounts.alliance;
    a.paused = paused;
    emit!(AlliancePausedSet {
        alliance: a.key(),
        paused,
    });
    Ok(())
}

pub fn handle_set_alliance_params(
    ctx: Context<AllianceAuthorityOnly>,
    fee_bps: u16,
    min_rate_bps: u32,
    max_rate_bps: u32,
) -> Result<()> {
    require!(fee_bps <= MAX_ALLIANCE_FEE_BPS, VestaError::ValueTooLarge);
    require!(
        max_rate_bps == 0 || min_rate_bps <= max_rate_bps,
        VestaError::InvalidSwapRate
    );
    let a = &mut ctx.accounts.alliance;
    a.fee_bps = fee_bps;
    a.min_rate_bps = min_rate_bps;
    a.max_rate_bps = max_rate_bps;
    emit!(AllianceParamsSet {
        alliance: a.key(),
        fee_bps,
        min_rate_bps,
        max_rate_bps,
    });
    Ok(())
}

pub fn handle_update_alliance_profile(
    ctx: Context<AllianceAuthorityOnly>,
    category: u8,
    metadata_uri: String,
) -> Result<()> {
    require!(
        metadata_uri.len() <= MAX_METADATA_URI_LEN,
        VestaError::StringTooLong
    );
    let a = &mut ctx.accounts.alliance;
    a.category = category;
    a.metadata_uri = metadata_uri;
    Ok(())
}

/// Alliance authority suspends / reactivates a member without forcing a full
/// leave — an inactive member cannot be a swap leg (checked in `swap_points`).
#[derive(Accounts)]
pub struct SetMemberActive<'info> {
    pub authority: Signer<'info>,

    #[account(has_one = authority @ VestaError::Unauthorized)]
    pub alliance: Account<'info, Alliance>,

    #[account(
        mut,
        seeds = [MEMBER_SEED, alliance.key().as_ref(), member.merchant.as_ref()],
        bump = member.bump,
    )]
    pub member: Account<'info, AllianceMember>,
}

pub fn handle_set_member_active(ctx: Context<SetMemberActive>, active: bool) -> Result<()> {
    let member = &mut ctx.accounts.member;
    member.active = active;
    emit!(MemberActiveSet {
        alliance: ctx.accounts.alliance.key(),
        merchant: member.merchant,
        active,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct JoinAlliance<'info> {
    #[account(mut)]
    pub merchant_authority: Signer<'info>,

    /// Handshake: unilateral joins cannot spoof rates (spec §3.6).
    pub alliance_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant_authority.key().as_ref()],
        bump = merchant.bump,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        mut,
        constraint = alliance.authority == alliance_authority.key() @ VestaError::Unauthorized,
    )]
    pub alliance: Account<'info, Alliance>,

    #[account(
        init,
        payer = merchant_authority,
        space = 8 + AllianceMember::INIT_SPACE,
        seeds = [MEMBER_SEED, alliance.key().as_ref(), merchant.key().as_ref()],
        bump,
    )]
    pub member: Account<'info, AllianceMember>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    pub system_program: Program<'info, System>,
}

pub fn handle_join_alliance(
    ctx: Context<JoinAlliance>,
    rate_bps_to_alliance: u32,
    swap_in_budget_raw: u64,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(rate_bps_to_alliance > 0, VestaError::InvalidSwapRate);
    check_rate_bounds(&ctx.accounts.alliance, rate_bps_to_alliance)?;
    require!(
        ctx.accounts.merchant.joined_alliance.is_none(),
        VestaError::AlreadyInAlliance
    );

    let member = &mut ctx.accounts.member;
    member.alliance = ctx.accounts.alliance.key();
    member.merchant = ctx.accounts.merchant.key();
    member.rate_bps_to_alliance = rate_bps_to_alliance;
    member.swap_in_budget_raw = swap_in_budget_raw;
    member.swapped_in_today = 0;
    member.budget_day = 0;
    member.active = true;
    member.joined_at = Clock::get()?.unix_timestamp;
    member.total_swapped_in = 0;
    member.total_swapped_out = 0;
    member.bump = ctx.bumps.member;

    let alliance = &mut ctx.accounts.alliance;
    alliance.member_count = alliance
        .member_count
        .checked_add(1)
        .ok_or(VestaError::Overflow)?;
    ctx.accounts.merchant.joined_alliance = Some(alliance.key());

    emit!(AllianceJoined {
        alliance: alliance.key(),
        merchant: member.merchant,
        rate_bps: rate_bps_to_alliance,
        swap_in_budget: swap_in_budget_raw,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct LeaveAlliance<'info> {
    #[account(mut)]
    pub merchant_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant_authority.key().as_ref()],
        bump = merchant.bump,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(mut)]
    pub alliance: Account<'info, Alliance>,

    #[account(
        mut,
        close = merchant_authority,
        seeds = [MEMBER_SEED, alliance.key().as_ref(), merchant.key().as_ref()],
        bump = member.bump,
    )]
    pub member: Account<'info, AllianceMember>,
}

pub fn handle_leave_alliance(ctx: Context<LeaveAlliance>) -> Result<()> {
    let alliance = &mut ctx.accounts.alliance;
    alliance.member_count = alliance.member_count.saturating_sub(1);
    ctx.accounts.merchant.joined_alliance = None;

    emit!(AllianceLeft {
        alliance: alliance.key(),
        merchant: ctx.accounts.merchant.key(),
    });
    Ok(())
}

#[derive(Accounts)]
pub struct SetSwapRate<'info> {
    pub merchant_authority: Signer<'info>,

    /// Anti-manipulation: rate changes need the alliance authority too.
    pub alliance_authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, merchant_authority.key().as_ref()],
        bump = merchant.bump,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(constraint = alliance.authority == alliance_authority.key() @ VestaError::Unauthorized)]
    pub alliance: Account<'info, Alliance>,

    #[account(
        mut,
        seeds = [MEMBER_SEED, alliance.key().as_ref(), merchant.key().as_ref()],
        bump = member.bump,
    )]
    pub member: Account<'info, AllianceMember>,
}

pub fn handle_set_swap_rate(ctx: Context<SetSwapRate>, new_rate: u32) -> Result<()> {
    require!(new_rate > 0, VestaError::InvalidSwapRate);
    check_rate_bounds(&ctx.accounts.alliance, new_rate)?;
    let member = &mut ctx.accounts.member;
    let old = member.rate_bps_to_alliance;
    member.rate_bps_to_alliance = new_rate;

    emit!(SwapRateSet {
        member: member.key(),
        old,
        new: new_rate,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct SetSwapBudget<'info> {
    pub merchant_authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, merchant_authority.key().as_ref()],
        bump = merchant.bump,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        mut,
        seeds = [MEMBER_SEED, member.alliance.as_ref(), merchant.key().as_ref()],
        bump = member.bump,
    )]
    pub member: Account<'info, AllianceMember>,
}

pub fn handle_set_swap_budget(ctx: Context<SetSwapBudget>, new_budget: u64) -> Result<()> {
    let member = &mut ctx.accounts.member;
    let old = member.swap_in_budget_raw;
    member.swap_in_budget_raw = new_budget;

    emit!(SwapBudgetSet {
        member: member.key(),
        old,
        new: new_budget,
    });
    Ok(())
}

/// UI-denominated swap (spec §3.6): raw units are NOT comparable across
/// mints — interest scaling runs from each mint's initialization timestamp,
/// so both legs convert through UI value via the verified shared path.
#[derive(Accounts)]
pub struct SwapPoints<'info> {
    #[account(mut)]
    pub customer: Signer<'info>,

    #[account(mut)]
    pub alliance: Box<Account<'info, Alliance>>,

    #[account(
        mut,
        seeds = [MEMBER_SEED, alliance.key().as_ref(), merchant_a.key().as_ref()],
        bump = member_a.bump,
        constraint = member_a.active @ VestaError::MemberInactive,
    )]
    pub member_a: Box<Account<'info, AllianceMember>>,

    #[account(
        mut,
        seeds = [MEMBER_SEED, alliance.key().as_ref(), merchant_b.key().as_ref()],
        bump = member_b.bump,
        constraint = member_b.active @ VestaError::MemberInactive,
    )]
    pub member_b: Box<Account<'info, AllianceMember>>,

    pub merchant_a: Box<Account<'info, Merchant>>,
    pub merchant_b: Box<Account<'info, Merchant>>,

    #[account(
        mut,
        seeds = [MINT_SEED, merchant_a.key().as_ref()],
        bump = merchant_a.mint_bump,
    )]
    pub mint_a: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [MINT_SEED, merchant_b.key().as_ref()],
        bump = merchant_b.mint_bump,
    )]
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = customer,
        associated_token::token_program = token_program,
    )]
    pub customer_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = customer,
        associated_token::mint = mint_b,
        associated_token::authority = customer,
        associated_token::token_program = token_program,
    )]
    pub customer_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,

    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_swap_points(
    ctx: Context<SwapPoints>,
    ui_amount: u64,
    max_raw_in: u64,
    min_raw_out: u64,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(!ctx.accounts.alliance.paused, VestaError::AlliancePaused);
    require!(ui_amount > 0, VestaError::InvalidAmount);

    // Leg A: how much raw the customer burns for this UI value.
    let raw_in = crate::util::ui_points_to_raw(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.mint_a.to_account_info(),
        ui_amount,
    )?;
    require!(raw_in > 0, VestaError::InvalidAmount);
    require!(raw_in <= max_raw_in, VestaError::SlippageExceeded);

    // Cross-rate in UI space, floor.
    let ui_out = u128::from(ui_amount)
        .checked_mul(u128::from(ctx.accounts.member_a.rate_bps_to_alliance))
        .and_then(|v| v.checked_div(u128::from(ctx.accounts.member_b.rate_bps_to_alliance)))
        .ok_or(VestaError::Overflow)?;
    // Alliance spread: haircut the output UI value by `fee_bps` (anti-abuse /
    // alliance monetization stub — the spread is simply not minted).
    let fee_bps = u128::from(ctx.accounts.alliance.fee_bps);
    let ui_out = ui_out
        .checked_mul(10_000u128.checked_sub(fee_bps).ok_or(VestaError::Overflow)?)
        .and_then(|v| v.checked_div(10_000))
        .ok_or(VestaError::Overflow)?;
    let ui_out = u64::try_from(ui_out).map_err(|_| VestaError::Overflow)?;
    require!(ui_out > 0, VestaError::InvalidAmount);

    // Leg B: how much raw that UI value mints on the destination mint.
    let raw_out = crate::util::ui_points_to_raw(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.mint_b.to_account_info(),
        ui_out,
    )?;
    require!(raw_out >= min_raw_out, VestaError::SlippageExceeded);

    // The koinon risk boundary: member B's self-chosen daily inbound budget.
    let today = u32::try_from(Clock::get()?.unix_timestamp / SECONDS_PER_DAY)
        .map_err(|_| VestaError::Overflow)?;
    let member_b = &mut ctx.accounts.member_b;
    if member_b.budget_day != today {
        member_b.budget_day = today;
        member_b.swapped_in_today = 0;
    }
    member_b.swapped_in_today = member_b
        .swapped_in_today
        .checked_add(raw_out)
        .ok_or(VestaError::Overflow)?;
    require!(
        member_b.swapped_in_today <= member_b.swap_in_budget_raw,
        VestaError::SwapBudgetExceeded
    );

    // Atomic: burn A (customer signs), mint B (merchant-B PDA signs).
    burn(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            Burn {
                mint: ctx.accounts.mint_a.to_account_info(),
                from: ctx.accounts.customer_ata_a.to_account_info(),
                authority: ctx.accounts.customer.to_account_info(),
            },
        ),
        raw_in,
    )?;
    let authority_b = ctx.accounts.merchant_b.authority;
    let merchant_b_seeds: &[&[u8]] = &[
        MERCHANT_SEED,
        authority_b.as_ref(),
        &[ctx.accounts.merchant_b.bump],
    ];
    mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            MintTo {
                mint: ctx.accounts.mint_b.to_account_info(),
                to: ctx.accounts.customer_ata_b.to_account_info(),
                authority: ctx.accounts.merchant_b.to_account_info(),
            },
            &[merchant_b_seeds],
        ),
        raw_out,
    )?;

    // Volume stats (member + alliance).
    let member_b = &mut ctx.accounts.member_b;
    member_b.total_swapped_in = member_b.total_swapped_in.saturating_add(raw_out);
    let member_a = &mut ctx.accounts.member_a;
    member_a.total_swapped_out = member_a.total_swapped_out.saturating_add(raw_in);
    let alliance = &mut ctx.accounts.alliance;
    alliance.total_swaps = alliance.total_swaps.saturating_add(1);
    alliance.total_ui_volume = alliance.total_ui_volume.saturating_add(u128::from(ui_amount));

    emit!(PointsSwapped {
        customer: ctx.accounts.customer.key(),
        merchant_a: ctx.accounts.merchant_a.key(),
        merchant_b: ctx.accounts.merchant_b.key(),
        ui_amount,
        raw_in,
        raw_out,
    });
    Ok(())
}
