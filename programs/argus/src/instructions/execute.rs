use anchor_lang::prelude::*;
use spl_token_2022_interface::extension::{
    permanent_delegate::PermanentDelegate, BaseStateWithExtensions, StateWithExtensions,
};

use crate::{
    constants::{DAILY_GIFT_CAP_RAW, LEDGER_SEED, SECONDS_PER_DAY},
    error::GuardError,
    events::{ClawbackObserved, PointsGifted},
    state::GiftLedger,
};

/// Accounts arrive privilege-de-escalated (read-only, non-signer) except the
/// hook-owned GiftLedger, which the meta list declares writable. The ledger is
/// typed as UncheckedAccount on purpose: rules 1–2 must succeed for wallets
/// that never opened a ledger (spec §4.3).
#[derive(Accounts)]
pub struct Execute<'info> {
    /// CHECK: source token account, validated by Token-2022 before the CPI.
    pub source: UncheckedAccount<'info>,
    /// CHECK: the transferring mint.
    pub mint: UncheckedAccount<'info>,
    /// CHECK: destination token account.
    pub destination: UncheckedAccount<'info>,
    /// CHECK: transfer authority (owner or delegate — or the permanent delegate).
    pub authority: UncheckedAccount<'info>,
    /// CHECK: the ExtraAccountMetaList PDA; Token-2022 resolved extras against it.
    #[account(seeds = [crate::constants::EXTRA_ACCOUNT_METAS_SEED, mint.key().as_ref()], bump)]
    pub extra_account_meta_list: UncheckedAccount<'info>,
    /// CHECK: rule-3 state; deserialized only on the peer-gift path.
    #[account(mut)]
    pub gift_ledger: UncheckedAccount<'info>,
    /// CHECK: destination owner wallet, dereferenced via pubkey-data meta.
    pub destination_owner: UncheckedAccount<'info>,
    /// CHECK: merchant treasury ATA, pinned as a literal meta at guard init.
    pub treasury: UncheckedAccount<'info>,
}

pub fn handle_execute(ctx: Context<Execute>, amount: u64) -> Result<()> {
    let source_owner = token_account_owner(&ctx.accounts.source)?;

    // Rule 1: the permanent delegate (merchant PDA) moves funds — clawback /
    // treasury operations. Read the delegate straight from the mint TLV.
    let mint_data = ctx.accounts.mint.try_borrow_data()?;
    let mint_state =
        StateWithExtensions::<spl_token_2022_interface::state::Mint>::unpack(&mint_data)
            .map_err(|_| GuardError::MintMismatch)?;
    if let Ok(pd) = mint_state.get_extension::<PermanentDelegate>() {
        if let Some(delegate) = Option::<Pubkey>::from(pd.delegate) {
            if delegate == ctx.accounts.authority.key() {
                emit!(ClawbackObserved {
                    mint: ctx.accounts.mint.key(),
                    source_owner,
                    amount,
                });
                return Ok(());
            }
        }
    }
    drop(mint_data);

    // Rule 2: paying the merchant — destination is the pinned treasury ATA.
    if ctx.accounts.destination.key() == ctx.accounts.treasury.key() {
        return Ok(());
    }

    // Rule 3a: best-effort program-owned-destination filter. Verify the
    // dereferenced wallet really is the destination's owner, then inspect
    // its owning program.
    let destination_owner_key = token_account_owner(&ctx.accounts.destination)?;
    require_keys_eq!(
        destination_owner_key,
        ctx.accounts.destination_owner.key(),
        GuardError::MetaListMismatch
    );
    require_keys_eq!(
        *ctx.accounts.destination_owner.owner,
        anchor_lang::system_program::ID,
        GuardError::ProgramOwnedDestination
    );

    // Rule 3b: the load-bearing daily velocity cap.
    let ledger_info = ctx.accounts.gift_ledger.to_account_info();
    let expected_ledger = Pubkey::find_program_address(
        &[
            LEDGER_SEED,
            ctx.accounts.mint.key().as_ref(),
            source_owner.as_ref(),
        ],
        &crate::ID,
    )
    .0;
    require_keys_eq!(
        ledger_info.key(),
        expected_ledger,
        GuardError::MetaListMismatch
    );
    require_keys_eq!(*ledger_info.owner, crate::ID, GuardError::LedgerNotOpened);

    let mut data = ledger_info.try_borrow_mut_data()?;
    let mut ledger =
        GiftLedger::try_deserialize(&mut data.as_ref()).map_err(|_| GuardError::LedgerNotOpened)?;

    let today = u32::try_from(Clock::get()?.unix_timestamp / SECONDS_PER_DAY)
        .map_err(|_| GuardError::Overflow)?;
    if ledger.day != today {
        ledger.day = today;
        ledger.gifted_today = 0;
    }
    ledger.gifted_today = ledger
        .gifted_today
        .checked_add(amount)
        .ok_or(GuardError::Overflow)?;
    require!(
        ledger.gifted_today <= DAILY_GIFT_CAP_RAW,
        GuardError::GiftCapExceeded
    );

    let mut cursor: &mut [u8] = &mut data;
    ledger.try_serialize(&mut cursor)?;

    emit!(PointsGifted {
        mint: ctx.accounts.mint.key(),
        source_owner,
        destination: ctx.accounts.destination.key(),
        amount,
        gifted_today: ledger.gifted_today,
    });
    Ok(())
}

/// The owner field of an SPL token account lives at bytes 32..64.
fn token_account_owner(account: &UncheckedAccount) -> Result<Pubkey> {
    let data = account.try_borrow_data()?;
    let bytes: [u8; 32] = data
        .get(32..64)
        .and_then(|s| s.try_into().ok())
        .ok_or(GuardError::MetaListMismatch)?;
    Ok(Pubkey::new_from_array(bytes))
}
