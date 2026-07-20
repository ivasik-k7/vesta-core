use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};
use anchor_spl::{
    token_2022::Token2022,
    token_2022_extensions::{
        interest_bearing_mint_update_rate, token_metadata_update_field,
        InterestBearingMintUpdateRate, TokenMetadataUpdateField,
    },
};
use spl_token_metadata_interface::state::Field;

use crate::{
    constants::{MAX_URI_LEN, MERCHANT_SEED, MINT_SEED},
    error::VestaError,
    events::{DecayRateUpdated, TokenAttributeSet, TokenMetadataUpdated},
    state::Merchant,
};

/// Max length for a custom attribute key or value, bytes.
pub const MAX_ATTR_LEN: usize = 64;

/// Core metadata field selectors for `update_token_metadata`.
pub mod metadata_field {
    pub const NAME: u8 = 0;
    pub const SYMBOL: u8 = 1;
    pub const URI: u8 = 2;
}

/// Attach or update a custom `additional_metadata` key/value on the point
/// token's on-chain metadata (Token-2022 TokenMetadata). This enriches the
/// token — tier, region, campaign tags surface directly in wallets/explorers —
/// without touching the immutable extension set, so no re-registration.
#[derive(Accounts)]
pub struct SetTokenAttribute<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    /// CHECK: the point mint (self-hosted metadata); PDA-bound, update authority
    /// is the merchant PDA. Written by the Token-2022 metadata update.
    #[account(
        mut,
        seeds = [MINT_SEED, merchant.key().as_ref()],
        bump = merchant.mint_bump,
        address = merchant.point_mint @ VestaError::MintMismatch,
    )]
    pub point_mint: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

pub fn handle_set_token_attribute(
    ctx: Context<SetTokenAttribute>,
    key: String,
    value: String,
) -> Result<()> {
    require!(
        !key.is_empty() && key.len() <= MAX_ATTR_LEN && value.len() <= MAX_ATTR_LEN,
        VestaError::StringTooLong
    );

    // Token-2022 UpdateField reallocs the metadata account but does not fund it;
    // pre-fund the rent for the growth (over-estimated; surplus stays in-account).
    let mint_ai = ctx.accounts.point_mint.to_account_info();
    let grow = key
        .len()
        .saturating_add(value.len())
        .saturating_add(16);
    let new_len = mint_ai.data_len().saturating_add(grow);
    let needed = Rent::get()?.minimum_balance(new_len);
    let delta = needed.saturating_sub(mint_ai.lamports());
    if delta > 0 {
        transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.authority.to_account_info(),
                    to: mint_ai.clone(),
                },
            ),
            delta,
        )?;
    }

    let merchant_key = ctx.accounts.merchant.key();
    let authority_key = ctx.accounts.authority.key();
    let id_bytes = ctx.accounts.merchant.id.to_le_bytes();
    let merchant_seeds: &[&[u8]] =
        &[MERCHANT_SEED, authority_key.as_ref(), &id_bytes, &[ctx.accounts.merchant.bump]];

    token_metadata_update_field(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TokenMetadataUpdateField {
                program_id: ctx.accounts.token_program.to_account_info(),
                metadata: mint_ai.clone(),
                update_authority: ctx.accounts.merchant.to_account_info(),
            },
            &[merchant_seeds],
        ),
        Field::Key(key.clone()),
        value.clone(),
    )?;

    emit!(TokenAttributeSet {
        merchant: merchant_key,
        mint: ctx.accounts.point_mint.key(),
        key,
        value,
    });
    Ok(())
}

/// Update a core token metadata field (name / symbol / uri) — enables merchant
/// rebrands post-registration. Owner only (reuses the SetTokenAttribute ctx).
pub fn handle_update_token_metadata(
    ctx: Context<SetTokenAttribute>,
    field_kind: u8,
    value: String,
) -> Result<()> {
    require!(value.len() <= MAX_URI_LEN, VestaError::StringTooLong);
    let field = match field_kind {
        metadata_field::NAME => Field::Name,
        metadata_field::SYMBOL => Field::Symbol,
        metadata_field::URI => Field::Uri,
        _ => return err!(VestaError::InvalidAmount),
    };

    // Pre-fund any realloc growth (surplus stays in-account).
    let mint_ai = ctx.accounts.point_mint.to_account_info();
    let new_len = mint_ai.data_len().saturating_add(value.len()).saturating_add(16);
    let needed = Rent::get()?.minimum_balance(new_len);
    let delta = needed.saturating_sub(mint_ai.lamports());
    if delta > 0 {
        transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.authority.to_account_info(),
                    to: mint_ai.clone(),
                },
            ),
            delta,
        )?;
    }

    let authority_key = ctx.accounts.authority.key();
    let id_bytes = ctx.accounts.merchant.id.to_le_bytes();
    let merchant_seeds: &[&[u8]] =
        &[MERCHANT_SEED, authority_key.as_ref(), &id_bytes, &[ctx.accounts.merchant.bump]];
    token_metadata_update_field(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TokenMetadataUpdateField {
                program_id: ctx.accounts.token_program.to_account_info(),
                metadata: mint_ai.clone(),
                update_authority: ctx.accounts.merchant.to_account_info(),
            },
            &[merchant_seeds],
        ),
        field,
        value,
    )?;

    emit!(TokenMetadataUpdated {
        merchant: ctx.accounts.merchant.key(),
        mint: ctx.accounts.point_mint.key(),
        field_kind,
    });
    Ok(())
}

/// Update the point token's interest-bearing (decay) rate. The merchant PDA is
/// the rate authority (set at registration). Owner only.
pub fn handle_update_decay_rate(ctx: Context<SetTokenAttribute>, new_rate_bps: i16) -> Result<()> {
    require!(
        (-10_000..=0).contains(&new_rate_bps),
        VestaError::InvalidDecayRate
    );
    let authority_key = ctx.accounts.authority.key();
    let id_bytes = ctx.accounts.merchant.id.to_le_bytes();
    let merchant_seeds: &[&[u8]] =
        &[MERCHANT_SEED, authority_key.as_ref(), &id_bytes, &[ctx.accounts.merchant.bump]];
    interest_bearing_mint_update_rate(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            InterestBearingMintUpdateRate {
                token_program_id: ctx.accounts.token_program.to_account_info(),
                mint: ctx.accounts.point_mint.to_account_info(),
                rate_authority: ctx.accounts.merchant.to_account_info(),
            },
            &[merchant_seeds],
        ),
        new_rate_bps,
    )?;

    let merchant = &mut ctx.accounts.merchant;
    merchant.decay_rate_bps = new_rate_bps;
    emit!(DecayRateUpdated {
        merchant: merchant.key(),
        new_rate_bps,
    });
    Ok(())
}
