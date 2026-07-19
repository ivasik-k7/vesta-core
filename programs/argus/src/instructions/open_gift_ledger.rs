use anchor_lang::prelude::*;

use crate::{
    constants::LEDGER_SEED, error::GuardError, events::GiftLedgerOpened, state::GiftLedger,
};

#[derive(Accounts)]
pub struct OpenGiftLedger<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: only used as a PDA seed; must be a Token-2022 mint account.
    pub mint: UncheckedAccount<'info>,

    #[account(
        init,
        payer = owner,
        space = 8 + GiftLedger::INIT_SPACE,
        seeds = [LEDGER_SEED, mint.key().as_ref(), owner.key().as_ref()],
        bump,
    )]
    pub gift_ledger: Account<'info, GiftLedger>,

    pub system_program: Program<'info, System>,
}

pub fn handle_open_gift_ledger(ctx: Context<OpenGiftLedger>) -> Result<()> {
    require_keys_eq!(
        *ctx.accounts.mint.owner,
        spl_token_2022_interface::ID,
        GuardError::MintMismatch
    );

    let ledger = &mut ctx.accounts.gift_ledger;
    ledger.day = 0;
    ledger.gifted_today = 0;
    ledger.bump = ctx.bumps.gift_ledger;

    emit!(GiftLedgerOpened {
        mint: ctx.accounts.mint.key(),
        owner: ctx.accounts.owner.key(),
    });
    Ok(())
}
