use anchor_lang::{
    prelude::*,
    system_program::{
        allocate, assign, create_account, transfer, Allocate, Assign, CreateAccount, Transfer,
    },
};
use anchor_spl::{
    associated_token::{self, AssociatedToken, Create},
    token_2022::{
        initialize_mint2, mint_to, set_authority, InitializeMint2, MintTo, SetAuthority, Token2022,
    },
    token_2022_extensions::{
        metadata_pointer_initialize, non_transferable_mint_initialize, token_metadata_initialize,
        MetadataPointerInitialize, NonTransferableMintInitialize, TokenMetadataInitialize,
    },
};
use spl_token_2022_interface::{extension::ExtensionType, instruction::AuthorityType};
use spl_token_metadata_interface::state::TokenMetadata;

use crate::{
    constants::{
        ACHIEVE_SEED, BADGE_SEED, CONFIG_SEED, CUSTOMER_SEED, KLEOS_SEED, MAX_NAME_LEN,
        MAX_URI_LEN, MERCHANT_SEED,
    },
    error::VestaError,
    events::{AchievementCreated, AchievementGranted},
    state::{Achievement, Config, CustomerProfile, KleosReceipt, Merchant},
};

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct CreateAchievement<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        init,
        payer = authority,
        space = 8 + Achievement::INIT_SPACE,
        seeds = [ACHIEVE_SEED, merchant.key().as_ref(), &id.to_le_bytes()],
        bump,
    )]
    pub achievement: Account<'info, Achievement>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    pub system_program: Program<'info, System>,
}

pub fn handle_create_achievement(
    ctx: Context<CreateAchievement>,
    id: u64,
    name: String,
    uri: String,
    threshold_lifetime: u64,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(name.len() <= MAX_NAME_LEN, VestaError::StringTooLong);
    require!(uri.len() <= MAX_URI_LEN, VestaError::StringTooLong);
    require!(threshold_lifetime > 0, VestaError::InvalidAmount);

    let achievement = &mut ctx.accounts.achievement;
    achievement.merchant = ctx.accounts.merchant.key();
    achievement.id = id;
    achievement.name = name;
    achievement.uri = uri;
    achievement.threshold_lifetime = threshold_lifetime;
    achievement.badge_count = 0;
    achievement.bump = ctx.bumps.achievement;

    emit!(AchievementCreated {
        merchant: achievement.merchant,
        id,
        threshold: threshold_lifetime,
    });
    Ok(())
}

/// Retire an achievement definition and reclaim its rent. Already-minted
/// soulbound badges are independent mints and are unaffected.
#[derive(Accounts)]
pub struct CloseAchievement<'info> {
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
    pub achievement: Account<'info, Achievement>,
}

pub fn handle_close_achievement(ctx: Context<CloseAchievement>) -> Result<()> {
    emit!(crate::events::AchievementClosed {
        merchant: ctx.accounts.merchant.key(),
        id: ctx.accounts.achievement.id,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct GrantAchievement<'info> {
    #[account(mut)]
    pub merchant_authority: Signer<'info>,

    // Seeds bind the PDA to the signer — the derivation IS the authorization.
    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant_authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(mut, has_one = merchant @ VestaError::MerchantMismatch)]
    pub achievement: Account<'info, Achievement>,

    /// CHECK: identity only; profile and badge PDAs derive from this key.
    pub customer: UncheckedAccount<'info>,

    #[account(
        seeds = [CUSTOMER_SEED, merchant.key().as_ref(), customer.key().as_ref()],
        bump = customer_profile.bump,
    )]
    pub customer_profile: Account<'info, CustomerProfile>,

    /// CHECK: created and initialized as the soulbound badge mint in the handler.
    #[account(mut, seeds = [BADGE_SEED, achievement.key().as_ref(), customer.key().as_ref()], bump)]
    pub badge_mint: UncheckedAccount<'info>,

    /// CHECK: created as the customer's badge ATA in the handler (the mint
    /// does not exist when Anchor constraints run).
    #[account(mut)]
    pub badge_ata: UncheckedAccount<'info>,

    /// The double-grant guard: init fails if it already exists — and it
    /// survives a holder-side badge burn.
    #[account(
        init,
        payer = merchant_authority,
        space = 8 + KleosReceipt::INIT_SPACE,
        seeds = [KLEOS_SEED, achievement.key().as_ref(), customer.key().as_ref()],
        bump,
    )]
    pub kleos_receipt: Account<'info, KleosReceipt>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_grant_achievement(ctx: Context<GrantAchievement>) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(
        ctx.accounts.customer_profile.lifetime_earned
            >= ctx.accounts.achievement.threshold_lifetime,
        VestaError::ThresholdNotReached
    );

    let merchant_key = ctx.accounts.merchant.key();
    let achievement_key = ctx.accounts.achievement.key();
    let customer_key = ctx.accounts.customer.key();
    let badge_key = ctx.accounts.badge_mint.key();
    let authority_key = ctx.accounts.merchant.authority;
    let id_bytes = ctx.accounts.merchant.id.to_le_bytes();

    let badge_seeds: &[&[u8]] = &[
        BADGE_SEED,
        achievement_key.as_ref(),
        customer_key.as_ref(),
        &[ctx.bumps.badge_mint],
    ];
    let merchant_seeds: &[&[u8]] = &[
        MERCHANT_SEED,
        authority_key.as_ref(),
        &id_bytes,
        &[ctx.accounts.merchant.bump],
    ];

    // Space for [NonTransferable, MetadataPointer]; rent pre-funded for the
    // metadata TLV realloc (§1.3 applies to badge mints too).
    let space =
        ExtensionType::try_calculate_account_len::<spl_token_2022_interface::state::Mint>(&[
            ExtensionType::NonTransferable,
            ExtensionType::MetadataPointer,
        ])
        .map_err(|_| VestaError::Overflow)?;
    let metadata = TokenMetadata {
        name: ctx.accounts.achievement.name.clone(),
        symbol: "KLEOS".to_string(),
        uri: ctx.accounts.achievement.uri.clone(),
        ..Default::default()
    };
    let metadata_space = metadata.tlv_size_of().map_err(|_| VestaError::Overflow)?;
    let lamports = Rent::get()?.minimum_balance(
        space
            .checked_add(metadata_space)
            .ok_or(VestaError::Overflow)?,
    );

    // Defensive creation — badge addresses are predictable too.
    let badge_info = ctx.accounts.badge_mint.to_account_info();
    if badge_info.lamports() == 0 {
        create_account(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                CreateAccount {
                    from: ctx.accounts.merchant_authority.to_account_info(),
                    to: badge_info.clone(),
                },
                &[badge_seeds],
            ),
            lamports,
            space as u64,
            &ctx.accounts.token_program.key(),
        )?;
    } else {
        let top_up = lamports.saturating_sub(badge_info.lamports());
        if top_up > 0 {
            transfer(
                CpiContext::new(
                    ctx.accounts.system_program.key(),
                    Transfer {
                        from: ctx.accounts.merchant_authority.to_account_info(),
                        to: badge_info.clone(),
                    },
                ),
                top_up,
            )?;
        }
        allocate(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                Allocate {
                    account_to_allocate: badge_info.clone(),
                },
                &[badge_seeds],
            ),
            space as u64,
        )?;
        assign(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                Assign {
                    account_to_assign: badge_info.clone(),
                },
                &[badge_seeds],
            ),
            &ctx.accounts.token_program.key(),
        )?;
    }

    let token_program = ctx.accounts.token_program.to_account_info();

    // NonTransferable strictly before initialize_mint2 (§3.5).
    non_transferable_mint_initialize(CpiContext::new(
        ctx.accounts.token_program.key(),
        NonTransferableMintInitialize {
            token_program_id: token_program.clone(),
            mint: badge_info.clone(),
        },
    ))?;
    metadata_pointer_initialize(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            MetadataPointerInitialize {
                token_program_id: token_program.clone(),
                mint: badge_info.clone(),
            },
        ),
        Some(merchant_key),
        Some(badge_key),
    )?;
    initialize_mint2(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            InitializeMint2 {
                mint: badge_info.clone(),
            },
        ),
        0,
        &merchant_key,
        None,
    )?;

    // Metadata TLV before any authority revocation — impossible afterwards (§3.5).
    token_metadata_initialize(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TokenMetadataInitialize {
                program_id: token_program.clone(),
                metadata: badge_info.clone(),
                update_authority: ctx.accounts.merchant.to_account_info(),
                mint_authority: ctx.accounts.merchant.to_account_info(),
                mint: badge_info.clone(),
            },
            &[merchant_seeds],
        ),
        metadata.name,
        metadata.symbol,
        metadata.uri,
    )?;

    // Badge ATA (Token-2022 ATAs auto-initialize ImmutableOwner), then 1-of-1.
    let expected_ata = associated_token::get_associated_token_address_with_program_id(
        &customer_key,
        &badge_key,
        &ctx.accounts.token_program.key(),
    );
    require_keys_eq!(
        ctx.accounts.badge_ata.key(),
        expected_ata,
        VestaError::TreasuryMismatch
    );
    associated_token::create(CpiContext::new(
        ctx.accounts.associated_token_program.key(),
        Create {
            payer: ctx.accounts.merchant_authority.to_account_info(),
            associated_token: ctx.accounts.badge_ata.to_account_info(),
            authority: ctx.accounts.customer.to_account_info(),
            mint: badge_info.clone(),
            system_program: ctx.accounts.system_program.to_account_info(),
            token_program: token_program.clone(),
        },
    ))?;
    mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            MintTo {
                mint: badge_info.clone(),
                to: ctx.accounts.badge_ata.to_account_info(),
                authority: ctx.accounts.merchant.to_account_info(),
            },
            &[merchant_seeds],
        ),
        1,
    )?;
    // Irreversible: supply can never increase again (FixedSupply on re-set).
    set_authority(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            SetAuthority {
                current_authority: ctx.accounts.merchant.to_account_info(),
                account_or_mint: badge_info.clone(),
            },
            &[merchant_seeds],
        ),
        AuthorityType::MintTokens,
        None,
    )?;

    let receipt = &mut ctx.accounts.kleos_receipt;
    receipt.granted_at = Clock::get()?.unix_timestamp;
    receipt.bump = ctx.bumps.kleos_receipt;

    let achievement = &mut ctx.accounts.achievement;
    achievement.badge_count = achievement
        .badge_count
        .checked_add(1)
        .ok_or(VestaError::Overflow)?;

    let merchant = &mut ctx.accounts.merchant;
    merchant.badges_issued = merchant.badges_issued.saturating_add(1);

    emit!(AchievementGranted {
        achievement: achievement_key,
        customer: customer_key,
        badge_mint: badge_key,
    });
    Ok(())
}
