use anchor_lang::{
    prelude::*,
    system_program::{
        allocate, assign, create_account, transfer, Allocate, Assign, CreateAccount, Transfer,
    },
};
use anchor_spl::{
    associated_token::{self, AssociatedToken, Create},
    token_2022::{initialize_mint2, InitializeMint2, Token2022},
    token_2022_extensions::{
        interest_bearing_mint_initialize, metadata_pointer_initialize,
        permanent_delegate_initialize, token_metadata_initialize, transfer_hook_initialize,
        InterestBearingMintInitialize, MetadataPointerInitialize, PermanentDelegateInitialize,
        TokenMetadataInitialize, TransferHookInitialize,
    },
};
use spl_token_2022_interface::extension::ExtensionType;
use spl_token_metadata_interface::state::TokenMetadata;

use crate::{
    constants::{
        CONFIG_SEED, DECIMALS, MAX_BASE_EARN_RATE, MAX_NAME_LEN, MAX_SYMBOL_LEN, MAX_URI_LEN,
        MERCHANT_SEED, MINT_SEED, MIN_BASE_EARN_RATE,
    },
    error::VestaError,
    events::{MerchantRegistered, MerchantUpdated},
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
pub struct RegisterMerchant<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + Merchant::INIT_SPACE,
        seeds = [MERCHANT_SEED, authority.key().as_ref()],
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

    let mint_seeds: &[&[u8]] = &[MINT_SEED, merchant_key.as_ref(), &[ctx.bumps.mint]];
    let merchant_seeds: &[&[u8]] = &[MERCHANT_SEED, authority_key.as_ref(), &[ctx.bumps.merchant]];

    // Space for the four extensions; rent pre-funded for the metadata TLV realloc too.
    let space =
        ExtensionType::try_calculate_account_len::<spl_token_2022_interface::state::Mint>(&[
            ExtensionType::MetadataPointer,
            ExtensionType::InterestBearingConfig,
            ExtensionType::TransferHook,
            ExtensionType::PermanentDelegate,
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
    merchant.authority = authority_key;
    merchant.point_mint = mint_key;
    merchant.treasury = expected_treasury;
    merchant.name = args.name.clone();
    merchant.decay_rate_bps = args.decay_rate_bps;
    merchant.base_earn_rate = args.base_earn_rate;
    merchant.lifetime_points_issued = 0;
    merchant.customer_count = 0;
    merchant.joined_alliance = None;
    merchant.bump = ctx.bumps.merchant;
    merchant.mint_bump = ctx.bumps.mint;

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
        seeds = [MERCHANT_SEED, authority.key().as_ref()],
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
