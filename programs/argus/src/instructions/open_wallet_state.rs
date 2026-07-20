use anchor_lang::prelude::*;

use crate::{
    constants::WALLET_STATE_SEED, error::GuardError, events::WalletStateOpened,
    state::WalletPolicyState,
};

/// One-time, customer-signed creation of the per-(mint, owner) velocity state.
/// In-hook creation is impossible (privilege de-escalation leaves no rent
/// payer), so first-time senders bundle this with their transfer (spec §3.6).
#[derive(Accounts)]
pub struct OpenWalletState<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: only used as a PDA seed; must be a Token-2022 mint account.
    pub mint: UncheckedAccount<'info>,

    #[account(
        init,
        payer = owner,
        space = 8 + WalletPolicyState::INIT_SPACE,
        seeds = [WALLET_STATE_SEED, mint.key().as_ref(), owner.key().as_ref()],
        bump,
    )]
    pub wallet_state: Account<'info, WalletPolicyState>,

    pub system_program: Program<'info, System>,
}

pub fn handle_open_wallet_state(ctx: Context<OpenWalletState>) -> Result<()> {
    require_keys_eq!(
        *ctx.accounts.mint.owner,
        spl_token_2022_interface::ID,
        GuardError::MintMismatch
    );

    let state = &mut ctx.accounts.wallet_state;
    state.day = 0;
    state.sent_today = 0;
    state.transfers_today = 0;
    state.last_transfer_at = 0;
    state.bump = ctx.bumps.wallet_state;

    emit!(WalletStateOpened {
        mint: ctx.accounts.mint.key(),
        owner: ctx.accounts.owner.key(),
    });
    Ok(())
}
