use anchor_lang::{
    prelude::*,
    system_program::{
        allocate, assign, create_account, transfer, Allocate, Assign, CreateAccount, Transfer,
    },
};
use anchor_spl::{
    associated_token::{self, AssociatedToken, Create},
    token_2022::{close_account, initialize_mint2, CloseAccount, InitializeMint2, Token2022},
    token_2022_extensions::{
        interest_bearing_mint_initialize, metadata_pointer_initialize,
        mint_close_authority_initialize, permanent_delegate_initialize, token_metadata_initialize,
        transfer_hook_initialize, InterestBearingMintInitialize, MetadataPointerInitialize,
        MintCloseAuthorityInitialize, PermanentDelegateInitialize, TokenMetadataInitialize,
        TransferHookInitialize,
    },
    token_interface::Mint,
};
use spl_token_2022_interface::extension::ExtensionType;
use spl_token_metadata_interface::state::TokenMetadata;

use crate::{
    constants::{
        CONFIG_SEED, DECIMALS, MAX_BASE_EARN_RATE, MAX_NAME_LEN, MAX_SYMBOL_LEN, MAX_URI_LEN,
        MERCHANT_SEED, MINT_SEED, MIN_BASE_EARN_RATE,
    },
    error::VestaError,
    events::{MerchantClosed, MerchantRegistered, MerchantUpdated},
    state::{Config, Merchant},
};

pub const ARGUS_ID: Pubkey = pubkey!("9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx");

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct RegisterMerchantArgs {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub decay_rate_bps: i16,
    pub base_earn_rate: u64,
    pub decimals: u8,
}

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct RegisterMerchant<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + Merchant::INIT_SPACE,
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &id.to_le_bytes()],
        bump,
    )]
    pub merchant: Account<'info, Merchant>,

    /// CHECK: created and initialized as a Token-2022 mint inside the handler;
    /// address is enforced by the PDA seeds.
    #[account(mut, seeds = [MINT_SEED, merchant.key().as_ref()], bump)]
    pub mint: UncheckedAccount<'info>,

    /// CHECK: created as the merchant treasury ATA inside the handler (the mint
    /// does not exist yet when Anchor constraints run); address verified there.
    #[account(mut)]
    pub treasury: UncheckedAccount<'info>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_register_merchant(
    ctx: Context<RegisterMerchant>,
    id: u64,
    args: RegisterMerchantArgs,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);
    require!(args.name.len() <= MAX_NAME_LEN, VestaError::StringTooLong);
    require!(
        args.symbol.len() <= MAX_SYMBOL_LEN,
        VestaError::StringTooLong
    );
    require!(args.uri.len() <= MAX_URI_LEN, VestaError::StringTooLong);
    require!(
        (-10_000..=0).contains(&args.decay_rate_bps),
        VestaError::InvalidDecayRate
    );
    require!(
        (MIN_BASE_EARN_RATE..=MAX_BASE_EARN_RATE).contains(&args.base_earn_rate),
        VestaError::InvalidEarnRate
    );
    require!(args.decimals == DECIMALS, VestaError::InvalidDecimals);

    let merchant_key = ctx.accounts.merchant.key();
    let authority_key = ctx.accounts.authority.key();
    let mint_key = ctx.accounts.mint.key();

    let id_bytes = id.to_le_bytes();
    let mint_seeds: &[&[u8]] = &[MINT_SEED, merchant_key.as_ref(), &[ctx.bumps.mint]];
    let merchant_seeds: &[&[u8]] = &[
        MERCHANT_SEED,
        authority_key.as_ref(),
        &id_bytes,
        &[ctx.bumps.merchant],
    ];

    // Space for the four extensions; rent pre-funded for the metadata TLV realloc too.
    let space =
        ExtensionType::try_calculate_account_len::<spl_token_2022_interface::state::Mint>(&[
            ExtensionType::MetadataPointer,
            ExtensionType::InterestBearingConfig,
            ExtensionType::TransferHook,
            ExtensionType::PermanentDelegate,
            ExtensionType::MintCloseAuthority,
        ])
        .map_err(|_| VestaError::Overflow)?;

    let metadata = TokenMetadata {
        name: args.name.clone(),
        symbol: args.symbol.clone(),
        uri: args.uri.clone(),
        ..Default::default()
    };
    let metadata_space = metadata.tlv_size_of().map_err(|_| VestaError::Overflow)?;

    let rent = Rent::get()?;
    let lamports = rent.minimum_balance(
        space
            .checked_add(metadata_space)
            .ok_or(VestaError::Overflow)?,
    );

    // Defensive creation: the mint address is publicly predictable, so a 1-lamport
    // donation must not brick registration (bare create_account fails on any
    // pre-funded address).
    let mint_info = ctx.accounts.mint.to_account_info();
    if mint_info.lamports() == 0 {
        create_account(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                CreateAccount {
                    from: ctx.accounts.authority.to_account_info(),
                    to: mint_info.clone(),
                },
                &[mint_seeds],
            ),
            lamports,
            space as u64,
            &ctx.accounts.token_program.key(),
        )?;
    } else {
        let top_up = lamports.saturating_sub(mint_info.lamports());
        if top_up > 0 {
            transfer(
                CpiContext::new(
                    ctx.accounts.system_program.key(),
                    Transfer {
                        from: ctx.accounts.authority.to_account_info(),
                        to: mint_info.clone(),
                    },
                ),
                top_up,
            )?;
        }
        allocate(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                Allocate {
                    account_to_allocate: mint_info.clone(),
                },
                &[mint_seeds],
            ),
            space as u64,
        )?;
        assign(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                Assign {
                    account_to_assign: mint_info.clone(),
                },
                &[mint_seeds],
            ),
            &ctx.accounts.token_program.key(),
        )?;
    }

    let token_program = ctx.accounts.token_program.to_account_info();

    // Extensions must be initialized before initialize_mint2.
    metadata_pointer_initialize(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            MetadataPointerInitialize {
                token_program_id: token_program.clone(),
                mint: mint_info.clone(),
            },
        ),
        Some(merchant_key),
        Some(mint_key),
    )?;
    interest_bearing_mint_initialize(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            InterestBearingMintInitialize {
                token_program_id: token_program.clone(),
                mint: mint_info.clone(),
            },
        ),
        Some(merchant_key),
        args.decay_rate_bps,
    )?;
    transfer_hook_initialize(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferHookInitialize {
                token_program_id: token_program.clone(),
                mint: mint_info.clone(),
            },
        ),
        Some(merchant_key),
        Some(ARGUS_ID),
    )?;
    permanent_delegate_initialize(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            PermanentDelegateInitialize {
                token_program_id: token_program.clone(),
                mint: mint_info.clone(),
            },
        ),
        &merchant_key,
    )?;
    // Close authority = merchant PDA → the mint can be closed once its supply
    // reaches zero, enabling a clean merchant delete (spec: full CRUD).
    mint_close_authority_initialize(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            MintCloseAuthorityInitialize {
                token_program_id: token_program.clone(),
                mint: mint_info.clone(),
            },
        ),
        Some(&merchant_key),
    )?;

    initialize_mint2(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            InitializeMint2 {
                mint: mint_info.clone(),
            },
        ),
        args.decimals,
        &merchant_key,
        None,
    )?;

    // Metadata TLV last — requires the mint authority (merchant PDA) signature.
    token_metadata_initialize(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TokenMetadataInitialize {
                program_id: token_program.clone(),
                metadata: mint_info.clone(),
                update_authority: ctx.accounts.merchant.to_account_info(),
                mint_authority: ctx.accounts.merchant.to_account_info(),
                mint: mint_info.clone(),
            },
            &[merchant_seeds],
        ),
        args.name.clone(),
        args.symbol,
        args.uri,
    )?;

    // Treasury = ATA(authority, mint); created here because the mint did not
    // exist when Anchor constraints ran.
    let expected_treasury =
        anchor_spl::associated_token::get_associated_token_address_with_program_id(
            &authority_key,
            &mint_key,
            &ctx.accounts.token_program.key(),
        );
    require_keys_eq!(
        ctx.accounts.treasury.key(),
        expected_treasury,
        VestaError::TreasuryMismatch
    );
    associated_token::create(CpiContext::new(
        ctx.accounts.associated_token_program.key(),
        Create {
            payer: ctx.accounts.authority.to_account_info(),
            associated_token: ctx.accounts.treasury.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
            mint: mint_info.clone(),
            system_program: ctx.accounts.system_program.to_account_info(),
            token_program: token_program.clone(),
        },
    ))?;

    let merchant = &mut ctx.accounts.merchant;
    merchant.id = id;
    merchant.authority = authority_key;
    merchant.point_mint = mint_key;
    merchant.treasury = expected_treasury;
    merchant.name = args.name.clone();
    merchant.decay_rate_bps = args.decay_rate_bps;
    merchant.base_earn_rate = args.base_earn_rate;
    merchant.lifetime_points_issued = 0;
    merchant.customer_count = 0;
    merchant.joined_alliance = None;
    merchant.operators = Default::default();
    merchant.operator_count = 0;
    merchant.paused = false;
    merchant.verified = false;
    merchant.category = 0;
    merchant.metadata_uri = String::new();
    merchant.lifetime_redemptions = 0;
    merchant.badges_issued = 0;
    merchant.lifetime_clawed_back = 0;
    merchant.clawback_count = 0;
    merchant.clawback_daily_cap_raw = 0;
    merchant.clawed_today = 0;
    merchant.clawback_day = 0;
    merchant.bump = ctx.bumps.merchant;
    merchant.mint_bump = ctx.bumps.mint;
    merchant.issue_status = crate::constants::issue_status::NORMAL;

    emit!(MerchantRegistered {
        merchant: merchant_key,
        mint: mint_key,
        name: args.name,
        decay_rate_bps: args.decay_rate_bps,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct UpdateMerchant<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
}

pub fn handle_update_merchant(
    ctx: Context<UpdateMerchant>,
    base_earn_rate: Option<u64>,
) -> Result<()> {
    require!(!ctx.accounts.config.paused, VestaError::ProtocolPaused);

    let merchant = &mut ctx.accounts.merchant;
    if let Some(rate) = base_earn_rate {
        require!(
            (MIN_BASE_EARN_RATE..=MAX_BASE_EARN_RATE).contains(&rate),
            VestaError::InvalidEarnRate
        );
        merchant.base_earn_rate = rate;
    }

    emit!(MerchantUpdated {
        merchant: merchant.key(),
        base_earn_rate: merchant.base_earn_rate,
    });
    Ok(())
}

/// Delete a merchant (full CRUD). Only when the point supply is zero — the mint
/// is closed (its close authority is the merchant PDA) and the Merchant account
/// is reclaimed. Guards against orphaning circulating points.
#[derive(Accounts)]
pub struct CloseMerchant<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        close = authority,
        seeds = [MERCHANT_SEED, authority.key().as_ref(), &merchant.id.to_le_bytes()],
        bump = merchant.bump,
        has_one = authority @ VestaError::Unauthorized,
        has_one = point_mint @ VestaError::MintMismatch,
    )]
    pub merchant: Account<'info, Merchant>,

    #[account(
        mut,
        seeds = [MINT_SEED, merchant.key().as_ref()],
        bump = merchant.mint_bump,
    )]
    pub point_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Program<'info, Token2022>,
}

pub fn handle_close_merchant(ctx: Context<CloseMerchant>) -> Result<()> {
    require!(
        ctx.accounts.point_mint.supply == 0,
        VestaError::MerchantNotEmpty
    );

    let authority_key = ctx.accounts.authority.key();
    let id_bytes = ctx.accounts.merchant.id.to_le_bytes();
    let merchant_seeds: &[&[u8]] = &[
        MERCHANT_SEED,
        authority_key.as_ref(),
        &id_bytes,
        &[ctx.accounts.merchant.bump],
    ];
    // Close the mint (supply is zero); rent → authority, merchant PDA signs as
    // the mint's close authority.
    close_account(CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        CloseAccount {
            account: ctx.accounts.point_mint.to_account_info(),
            destination: ctx.accounts.authority.to_account_info(),
            authority: ctx.accounts.merchant.to_account_info(),
        },
        &[merchant_seeds],
    ))?;

    emit!(MerchantClosed {
        merchant: ctx.accounts.merchant.key(),
        id: ctx.accounts.merchant.id,
        authority: authority_key,
    });
    Ok(())
}
