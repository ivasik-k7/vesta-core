use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_2022::{burn, Burn, Token2022},
    token_interface::{Mint, TokenAccount},
};

use crate::{
    constants::{CONFIG_SEED, CUSTOMER_SEED, MERCHANT_SEED, MINT_SEED, OFFER_SEED, RECEIPT_SEED},
    error::VestaError,
    events::{OfferClosed, OfferCreated, OfferRedeemed, OfferSegmentSet, ReceiptClosed},
    state::{Config, CustomerProfile, Merchant, Offer, Receipt},
};

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct CreateOffer<'info> {
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
        space = 8 + Offer::INIT_SPACE,
        seeds = [OFFER_SEED, merchant.key().as_ref(), &id.to_le_bytes()],
        bump,
    )]
    pub offer: Account<'info, Offer>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    pub system_program: Program<'info, System>,
}

pub fn handle_create_offer(
    ctx: Context<CreateOffer>,
    id: u64,
    price_points: u64,
    supply: u32,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(!ctx.accounts.merchant.paused, VestaError::MerchantPaused);
    require!(
        ctx.accounts
            .merchant
            .may_manage(&ctx.accounts.authority.key()),
        VestaError::Unauthorized
    );
    require!(price_points > 0, VestaError::InvalidAmount);
    require!(supply > 0, VestaError::InvalidAmount);

    let offer = &mut ctx.accounts.offer;
    offer.merchant = ctx.accounts.merchant.key();
    offer.id = id;
    offer.price_points = price_points;
    offer.supply_remaining = supply;
    offer.active = true;
    offer.bump = ctx.bumps.offer;
    offer.required_segment = 0;

    emit!(OfferCreated {
        merchant: offer.merchant,
        offer_id: id,
        price_points,
        supply,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct CloseOffer<'info> {
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
    pub offer: Account<'info, Offer>,
}

pub fn handle_close_offer(ctx: Context<CloseOffer>) -> Result<()> {
    emit!(OfferClosed {
        merchant: ctx.accounts.merchant.key(),
        offer_id: ctx.accounts.offer.id,
    });
    Ok(())
}

/// Gate (or ungate) an offer on a verified segment (spec 12 §4.5). `0` = open;
/// else the redeemer must satisfy segment index `required_segment - 1`.
#[derive(Accounts)]
pub struct SetOfferSegment<'info> {
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, merchant.authority.as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(mut, has_one = merchant @ VestaError::MerchantMismatch)]
    pub offer: Account<'info, Offer>,
}

pub fn handle_set_offer_segment(ctx: Context<SetOfferSegment>, required_segment: u8) -> Result<()> {
    require!(
        ctx.accounts
            .merchant
            .may_manage(&ctx.accounts.authority.key()),
        VestaError::Unauthorized
    );
    require!(
        usize::from(required_segment) <= crate::constants::MAX_SEGMENTS,
        VestaError::InvalidSegment
    );
    ctx.accounts.offer.required_segment = required_segment;
    emit!(OfferSegmentSet {
        merchant: ctx.accounts.merchant.key(),
        offer_id: ctx.accounts.offer.id,
        required_segment,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct RedeemOffer<'info> {
    #[account(mut)]
    pub customer: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant.authority.as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Box<Account<'info, Merchant>>,

    #[account(mut, has_one = merchant @ VestaError::MerchantMismatch)]
    pub offer: Account<'info, Offer>,

    // init_if_needed: a customer holding only gifted points has no profile yet —
    // the gift-then-redeem path must not brick.
    #[account(
        init_if_needed,
        payer = customer,
        space = 8 + CustomerProfile::INIT_SPACE,
        seeds = [CUSTOMER_SEED, merchant.key().as_ref(), customer.key().as_ref()],
        bump,
    )]
    pub customer_profile: Account<'info, CustomerProfile>,

    #[account(
        init,
        payer = customer,
        space = 8 + Receipt::INIT_SPACE,
        seeds = [
            RECEIPT_SEED,
            offer.key().as_ref(),
            customer.key().as_ref(),
            &customer_profile.lifetime_redemptions.to_le_bytes(),
        ],
        bump,
    )]
    pub receipt: Account<'info, Receipt>,

    #[account(
        mut,
        seeds = [MINT_SEED, merchant.key().as_ref()],
        bump = merchant.mint_bump,
    )]
    pub point_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = point_mint,
        associated_token::authority = customer,
        associated_token::token_program = token_program,
    )]
    pub customer_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,

    /// Optional verified-segment gate accounts (spec 12 §4.5), required only when
    /// the offer sets `required_segment`.
    #[account(
        seeds = [crate::constants::SEGMENTS_SEED, merchant.key().as_ref()],
        bump = merchant_segments.bump,
    )]
    pub merchant_segments: Option<Box<Account<'info, crate::state::MerchantSegments>>>,

    #[account(
        seeds = [
            crate::constants::CUSTOMER_ELIGIBILITY_SEED,
            merchant.key().as_ref(),
            customer.key().as_ref(),
        ],
        bump = customer_eligibility.bump,
    )]
    pub customer_eligibility: Option<Box<Account<'info, crate::state::CustomerEligibility>>>,

    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_redeem_offer(ctx: Context<RedeemOffer>, max_raw_amount: u64) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(!ctx.accounts.merchant.paused, VestaError::MerchantPaused);

    // Verified-segment gate: an offer may restrict redemption to customers who
    // satisfy a segment (e.g. accredited-only, verified-region flash sale).
    let required_segment = ctx.accounts.offer.required_segment;
    if required_segment != 0 {
        let now = Clock::get()?.unix_timestamp;
        let ok = match (
            &ctx.accounts.merchant_segments,
            &ctx.accounts.customer_eligibility,
        ) {
            (Some(segs), Some(cel)) => {
                cel.satisfies(required_segment.saturating_sub(1), segs.policy_epoch, now)
            }
            _ => false,
        };
        require!(ok, VestaError::OfferSegmentGated);
    }

    let offer = &mut ctx.accounts.offer;
    require!(offer.active, VestaError::OfferInactive);
    require!(offer.supply_remaining > 0, VestaError::OfferSoldOut);

    // UI → raw on-chain via the shared verified path (spec §3.4).
    let raw_needed = crate::util::ui_points_to_raw(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.point_mint.to_account_info(),
        offer.price_points,
    )?;
    require!(raw_needed <= max_raw_amount, VestaError::SlippageExceeded);

    burn(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            Burn {
                mint: ctx.accounts.point_mint.to_account_info(),
                from: ctx.accounts.customer_ata.to_account_info(),
                authority: ctx.accounts.customer.to_account_info(),
            },
        ),
        raw_needed,
    )?;

    offer.supply_remaining = offer
        .supply_remaining
        .checked_sub(1)
        .ok_or(VestaError::OfferSoldOut)?;

    let first_touch = ctx.accounts.customer_profile.wallet == Pubkey::default();
    {
        let profile = &mut ctx.accounts.customer_profile;
        if first_touch {
            profile.wallet = ctx.accounts.customer.key();
            profile.merchant = ctx.accounts.merchant.key();
            profile.bump = ctx.bumps.customer_profile;
        }
        profile.lifetime_redemptions = profile
            .lifetime_redemptions
            .checked_add(1)
            .ok_or(VestaError::Overflow)?;
    }

    let merchant = &mut ctx.accounts.merchant;
    merchant.lifetime_redemptions = merchant.lifetime_redemptions.saturating_add(1);
    // Count a customer whose first-ever touch at this merchant is a redemption
    // (gift-then-redeem) so customer_count is not under-reported (AUDIT L-4).
    if first_touch {
        merchant.customer_count = merchant.customer_count.saturating_add(1);
    }

    let receipt = &mut ctx.accounts.receipt;
    receipt.offer = offer.key();
    receipt.customer = ctx.accounts.customer.key();
    receipt.redeemed_at = Clock::get()?.unix_timestamp;
    receipt.bump = ctx.bumps.receipt;

    emit!(OfferRedeemed {
        offer: offer.key(),
        customer: ctx.accounts.customer.key(),
        raw_burned: raw_needed,
        receipt: receipt.key(),
    });
    Ok(())
}

#[derive(Accounts)]
pub struct CloseReceipt<'info> {
    #[account(mut)]
    pub customer: Signer<'info>,

    #[account(
        mut,
        close = customer,
        has_one = customer @ VestaError::Unauthorized,
    )]
    pub receipt: Account<'info, Receipt>,
}

pub fn handle_close_receipt(ctx: Context<CloseReceipt>) -> Result<()> {
    emit!(ReceiptClosed {
        receipt: ctx.accounts.receipt.key(),
        customer: ctx.accounts.customer.key(),
    });
    Ok(())
}
