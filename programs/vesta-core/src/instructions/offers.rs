use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_2022::{burn, Burn, Token2022},
    token_interface::{Mint, TokenAccount},
};

use crate::{
    constants::{CONFIG_SEED, CUSTOMER_SEED, MERCHANT_SEED, MINT_SEED, OFFER_SEED, RECEIPT_SEED},
    error::VestaError,
    events::{OfferClosed, OfferCreated, OfferRedeemed, ReceiptClosed},
    state::{Config, CustomerProfile, Merchant, Offer, Receipt},
};

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct CreateOffer<'info> {
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
    require!(price_points > 0, VestaError::InvalidAmount);
    require!(supply > 0, VestaError::InvalidAmount);

    let offer = &mut ctx.accounts.offer;
    offer.merchant = ctx.accounts.merchant.key();
    offer.id = id;
    offer.price_points = price_points;
    offer.supply_remaining = supply;
    offer.active = true;
    offer.bump = ctx.bumps.offer;

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
    pub offer: Account<'info, Offer>,
}

pub fn handle_close_offer(ctx: Context<CloseOffer>) -> Result<()> {
    emit!(OfferClosed {
        merchant: ctx.accounts.merchant.key(),
        offer_id: ctx.accounts.offer.id,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct RedeemOffer<'info> {
    #[account(mut)]
    pub customer: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant.authority.as_ref()],
        bump = merchant.bump,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Account<'info, Merchant>,

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
    pub point_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
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

pub fn handle_redeem_offer(ctx: Context<RedeemOffer>, max_raw_amount: u64) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(!ctx.accounts.merchant.paused, VestaError::MerchantPaused);

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

    let profile = &mut ctx.accounts.customer_profile;
    if profile.wallet == Pubkey::default() {
        profile.wallet = ctx.accounts.customer.key();
        profile.merchant = ctx.accounts.merchant.key();
        profile.bump = ctx.bumps.customer_profile;
    }
    profile.lifetime_redemptions = profile
        .lifetime_redemptions
        .checked_add(1)
        .ok_or(VestaError::Overflow)?;

    let merchant = &mut ctx.accounts.merchant;
    merchant.lifetime_redemptions = merchant.lifetime_redemptions.saturating_add(1);

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
