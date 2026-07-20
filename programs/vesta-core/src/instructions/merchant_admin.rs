use anchor_lang::prelude::*;

use crate::{
    constants::{CONFIG_SEED, MAX_METADATA_URI_LEN, MERCHANT_SEED},
    error::VestaError,
    events::{
        ClawbackCapSet, MerchantOperatorSet, MerchantPausedSet, MerchantProfileUpdated,
        MerchantVerifiedSet,
    },
    state::{Config, Merchant, MAX_OPERATORS},
};

/// Owner-only merchant control surface.
#[derive(Accounts)]
pub struct MerchantOwnerOnly<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,
}

pub fn handle_set_merchant_operator(
    ctx: Context<MerchantOwnerOnly>,
    operator: Pubkey,
    add: bool,
) -> Result<()> {
    let m = &mut ctx.accounts.merchant;
    let count = usize::from(m.operator_count);
    if add {
        require!(count < MAX_OPERATORS, VestaError::OperatorsFull);
        require!(
            !m.operators[..count].contains(&operator),
            VestaError::OperatorsFull
        );
        m.operators[count] = operator;
        m.operator_count = m.operator_count.saturating_add(1);
    } else {
        let idx = m.operators[..count]
            .iter()
            .position(|k| *k == operator)
            .ok_or(VestaError::OperatorNotFound)?;
        // Swap-remove: move the last operator into the freed slot.
        let last = count.saturating_sub(1);
        m.operators[idx] = m.operators[last];
        m.operators[last] = Pubkey::default();
        m.operator_count = m.operator_count.saturating_sub(1);
    }
    emit!(MerchantOperatorSet {
        merchant: m.key(),
        operator,
        added: add,
    });
    Ok(())
}

pub fn handle_set_merchant_paused(ctx: Context<MerchantOwnerOnly>, paused: bool) -> Result<()> {
    let m = &mut ctx.accounts.merchant;
    m.paused = paused;
    emit!(MerchantPausedSet {
        merchant: m.key(),
        paused,
    });
    Ok(())
}

pub fn handle_update_merchant_profile(
    ctx: Context<MerchantOwnerOnly>,
    category: u8,
    metadata_uri: String,
) -> Result<()> {
    require!(
        metadata_uri.len() <= MAX_METADATA_URI_LEN,
        VestaError::StringTooLong
    );
    let m = &mut ctx.accounts.merchant;
    m.category = category;
    m.metadata_uri = metadata_uri;
    emit!(MerchantProfileUpdated {
        merchant: m.key(),
        category,
    });
    Ok(())
}

pub fn handle_set_clawback_cap(ctx: Context<MerchantOwnerOnly>, daily_cap_raw: u64) -> Result<()> {
    let m = &mut ctx.accounts.merchant;
    m.clawback_daily_cap_raw = daily_cap_raw;
    emit!(ClawbackCapSet {
        merchant: m.key(),
        daily_cap_raw,
    });
    Ok(())
}

/// Protocol-admin-set trust badge (e.g. KYB-verified brand).
#[derive(Accounts)]
pub struct VerifyMerchant<'info> {
    pub admin: Signer<'info>,

    // Re-derive the merchant PDA for defense in depth (AUDIT I-2), consistent
    // with every other instruction that touches a merchant.
    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant.authority.as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ VestaError::Unauthorized,
    )]
    pub config: Account<'info, Config>,
}

pub fn handle_verify_merchant(ctx: Context<VerifyMerchant>, verified: bool) -> Result<()> {
    let m = &mut ctx.accounts.merchant;
    m.verified = verified;
    emit!(MerchantVerifiedSet {
        merchant: m.key(),
        verified,
    });
    Ok(())
}
