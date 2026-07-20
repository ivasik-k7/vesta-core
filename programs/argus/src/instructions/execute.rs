use anchor_lang::prelude::*;
use spl_token_2022_interface::extension::{
    permanent_delegate::PermanentDelegate, BaseStateWithExtensions, StateWithExtensions,
};

use crate::{
    constants::{
        attestation_offset, flags, reason, ATTESTATION_DISCRIMINATOR, ATTESTATION_SEED, GUARD_SEED,
        LIST_ENTRY_SEED, SECONDS_PER_DAY, WALLET_STATE_SEED,
    },
    error::GuardError,
    events::TransferDecision,
    state::{GuardConfig, WalletPolicyState},
};

/// Accounts arrive privilege-de-escalated (read-only, non-signer) except the
/// hook-owned WalletPolicyState, which the meta list declares writable. State,
/// list, and attestation accounts are Unchecked on purpose: the short-circuit
/// rules (issuer/treasury) must succeed for wallets that never opened state.
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
    /// Per-mint policy (spec §2.1). Deserialization failure fails closed.
    #[account(seeds = [GUARD_SEED, mint.key().as_ref()], bump = guard_config.bump)]
    pub guard_config: Account<'info, GuardConfig>,
    /// CHECK: velocity state; deserialized only on the peer-gift path.
    #[account(mut)]
    pub wallet_state: UncheckedAccount<'info>,
    /// CHECK: destination owner wallet, dereferenced via pubkey-data meta.
    pub destination_owner: UncheckedAccount<'info>,
    /// CHECK: allow/deny list entry PDA; existence == membership.
    pub list_entry: UncheckedAccount<'info>,
    /// CHECK: aegis program id, pinned in the meta list.
    pub aegis_program: UncheckedAccount<'info>,
    /// CHECK: trusted aegis issuer, pinned in the meta list.
    pub aegis_issuer: UncheckedAccount<'info>,
    /// CHECK: aegis attestation PDA over the destination owner (spec §7).
    pub attestation: UncheckedAccount<'info>,
}

pub fn handle_execute(ctx: Context<Execute>, amount: u64) -> Result<()> {
    let a = &ctx.accounts;
    let mint_key = a.mint.key();
    let source_owner = token_account_owner(&a.source)?;

    // Rule 1: the permanent delegate (merchant PDA) moves funds — clawback /
    // treasury operations. Read the delegate straight from the mint TLV.
    {
        let mint_data = a.mint.try_borrow_data()?;
        let mint_state =
            StateWithExtensions::<spl_token_2022_interface::state::Mint>::unpack(&mint_data)
                .map_err(|_| GuardError::MintMismatch)?;
        if let Ok(pd) = mint_state.get_extension::<PermanentDelegate>() {
            if let Some(delegate) = Option::<Pubkey>::from(pd.delegate) {
                if delegate == a.authority.key() {
                    return decide(&ctx, source_owner, amount, true, reason::ISSUER_FLOW);
                }
            }
        }
    }

    // Rule 2: paying the merchant — destination is the treasury ATA.
    if a.destination.key() == a.guard_config.treasury {
        return decide(&ctx, source_owner, amount, true, reason::TREASURY_FLOW);
    }

    // Rule 3: per-mint peer freeze (issuer/treasury flows already passed).
    if a.guard_config.paused {
        decide(&ctx, source_owner, amount, false, reason::MINT_PAUSED)?;
        return err!(GuardError::MintPaused);
    }

    // Rule 4: a zero-amount transfer changes nothing.
    if amount == 0 {
        return decide(&ctx, source_owner, amount, true, reason::NOOP);
    }

    // Peer gift below here. Gifting can be hard-disabled.
    if a.guard_config.flags & flags::GIFTING_DISABLED != 0 {
        decide(&ctx, source_owner, amount, false, reason::GIFTING_DISABLED)?;
        return err!(GuardError::GiftingDisabled);
    }

    // Bind the dereferenced destination owner to the real token account owner.
    let destination_owner = token_account_owner(&a.destination)?;
    require_keys_eq!(
        destination_owner,
        a.destination_owner.key(),
        GuardError::MetaListMismatch
    );

    // Rule 5: best-effort program-owned-destination filter.
    if a.guard_config.flags & flags::BLOCK_PROGRAM_OWNED != 0
        && *a.destination_owner.owner != anchor_lang::system_program::ID
    {
        decide(&ctx, source_owner, amount, false, reason::PROGRAM_OWNED_DEST)?;
        return err!(GuardError::ProgramOwnedDestination);
    }

    // Rules 6–7: allow / deny lists (spec §2.4). Membership == entry exists.
    if a.guard_config.flags & (flags::ALLOWLIST_ONLY | flags::DENYLIST) != 0 {
        let expected_entry = Pubkey::find_program_address(
            &[LIST_ENTRY_SEED, mint_key.as_ref(), destination_owner.as_ref()],
            &crate::ID,
        )
        .0;
        require_keys_eq!(
            a.list_entry.key(),
            expected_entry,
            GuardError::MetaListMismatch
        );
        let listed = *a.list_entry.owner == crate::ID && !a.list_entry.data_is_empty();

        if a.guard_config.flags & flags::ALLOWLIST_ONLY != 0 && !listed {
            decide(&ctx, source_owner, amount, false, reason::NOT_ALLOWLISTED)?;
            return err!(GuardError::NotAllowlisted);
        }
        if a.guard_config.flags & flags::DENYLIST != 0 && listed {
            decide(&ctx, source_owner, amount, false, reason::DENY_LISTED)?;
            return err!(GuardError::DenyListed);
        }
    }

    // Rule 8: attestation gating (spec §7).
    if a.guard_config.flags & flags::REQUIRE_ATTESTATION != 0
        && !attestation_ok(&ctx, destination_owner)?
    {
        decide(&ctx, source_owner, amount, false, reason::ATTESTATION_FAILED)?;
        return err!(GuardError::AttestationFailed);
    }

    // Rule 9: per-transfer cap.
    let per_tx_cap = a.guard_config.per_tx_cap;
    if per_tx_cap != 0 && amount > per_tx_cap {
        decide(&ctx, source_owner, amount, false, reason::PER_TX_EXCEEDED)?;
        return err!(GuardError::PerTxExceeded);
    }

    // Rule 10: destination balance cap (anti-hoarding, receiving side). The
    // hook fires AFTER Token-2022 applies the transfer, so the destination
    // balance already includes `amount` — compare it directly.
    let max_balance = a.guard_config.max_wallet_balance;
    if max_balance != 0 {
        let dest_balance = token_account_amount(&a.destination)?;
        if dest_balance > max_balance {
            decide(&ctx, source_owner, amount, false, reason::BALANCE_CAP)?;
            return err!(GuardError::BalanceCapExceeded);
        }
    }

    // Rules 11–13 read/write the source owner's velocity state.
    let state_info = a.wallet_state.to_account_info();
    let expected_state = Pubkey::find_program_address(
        &[WALLET_STATE_SEED, mint_key.as_ref(), source_owner.as_ref()],
        &crate::ID,
    )
    .0;
    require_keys_eq!(
        state_info.key(),
        expected_state,
        GuardError::MetaListMismatch
    );
    require_keys_eq!(*state_info.owner, crate::ID, GuardError::StateNotOpened);

    let mut data = state_info.try_borrow_mut_data()?;
    let mut state = WalletPolicyState::try_deserialize(&mut data.as_ref())
        .map_err(|_| GuardError::StateNotOpened)?;

    let now = Clock::get()?.unix_timestamp;
    let today = u32::try_from(now / SECONDS_PER_DAY).map_err(|_| GuardError::Overflow)?;
    if state.day != today {
        state.day = today;
        state.sent_today = 0;
        state.transfers_today = 0;
    }

    // Rule 11: cooldown between transfers.
    let cooldown = i64::from(a.guard_config.cooldown_secs);
    if cooldown > 0 && state.last_transfer_at != 0 {
        let elapsed = now.checked_sub(state.last_transfer_at).unwrap_or(i64::MAX);
        if elapsed < cooldown {
            decide(&ctx, source_owner, amount, false, reason::COOLDOWN)?;
            return err!(GuardError::CooldownActive);
        }
    }

    // Rule 12: daily transfer-count cap.
    let count_cap = a.guard_config.transfers_per_day_cap;
    if count_cap != 0 && state.transfers_today >= count_cap {
        decide(&ctx, source_owner, amount, false, reason::TRANSFER_COUNT)?;
        return err!(GuardError::TransferCountExceeded);
    }

    // Rule 13: the load-bearing daily volume cap.
    let new_sent = state
        .sent_today
        .checked_add(amount)
        .ok_or(GuardError::Overflow)?;
    if new_sent > a.guard_config.daily_gift_cap {
        decide(&ctx, source_owner, amount, false, reason::DAILY_CAP)?;
        return err!(GuardError::GiftCapExceeded);
    }

    // Commit.
    state.sent_today = new_sent;
    state.transfers_today = state.transfers_today.saturating_add(1);
    state.last_transfer_at = now;
    let mut cursor: &mut [u8] = &mut data;
    state.try_serialize(&mut cursor)?;
    drop(data);

    decide(&ctx, source_owner, amount, true, reason::GIFT)
}

/// Emit the decision event, then return Ok. Callers append their own `err!`
/// after a reject-decide so the transfer reverts (the reason still shows in
/// the failed-tx logs — the audit trail, spec §10).
fn decide(
    ctx: &Context<Execute>,
    source_owner: Pubkey,
    amount: u64,
    allowed: bool,
    reason: u16,
) -> Result<()> {
    emit!(TransferDecision {
        mint: ctx.accounts.mint.key(),
        source_owner,
        destination_owner: ctx.accounts.destination_owner.key(),
        amount,
        allowed,
        reason,
    });
    Ok(())
}

/// Validate the aegis attestation on the destination owner against policy
/// (spec §7). argus does not link aegis; fields are read by verified offset.
fn attestation_ok(ctx: &Context<Execute>, destination_owner: Pubkey) -> Result<bool> {
    let a = &ctx.accounts;

    // The meta list pins these, but assert integrity before trusting them.
    require_keys_eq!(
        a.aegis_program.key(),
        crate::constants::AEGIS_ID,
        GuardError::MetaListMismatch
    );
    require_keys_eq!(
        a.aegis_issuer.key(),
        a.guard_config.attestation_issuer,
        GuardError::MetaListMismatch
    );
    let expected = Pubkey::find_program_address(
        &[
            ATTESTATION_SEED,
            a.guard_config.attestation_issuer.as_ref(),
            destination_owner.as_ref(),
        ],
        &crate::constants::AEGIS_ID,
    )
    .0;
    require_keys_eq!(a.attestation.key(), expected, GuardError::MetaListMismatch);

    // Missing / wrong-owner attestation → not satisfied (fail closed).
    if *a.attestation.owner != crate::constants::AEGIS_ID || a.attestation.data_is_empty() {
        return Ok(false);
    }
    let data = a.attestation.try_borrow_data()?;
    if data.len() < attestation_offset::MIN_LEN || data[..8] != ATTESTATION_DISCRIMINATOR {
        return Ok(false);
    }

    let issuer = Pubkey::try_from(&data[attestation_offset::ISSUER])
        .map_err(|_| GuardError::AttestationFailed)?;
    let subject = Pubkey::try_from(&data[attestation_offset::SUBJECT])
        .map_err(|_| GuardError::AttestationFailed)?;
    if issuer != a.guard_config.attestation_issuer || subject != destination_owner {
        return Ok(false);
    }

    let schema = u16::from_le_bytes(
        data[attestation_offset::SCHEMA]
            .try_into()
            .map_err(|_| GuardError::AttestationFailed)?,
    );
    if schema != a.guard_config.attestation_schema {
        return Ok(false);
    }

    let value = u64::from_le_bytes(
        data[attestation_offset::VALUE]
            .try_into()
            .map_err(|_| GuardError::AttestationFailed)?,
    );
    if value & a.guard_config.attestation_mask == 0 {
        return Ok(false);
    }

    let expires_at = i64::from_le_bytes(
        data[attestation_offset::EXPIRES_AT]
            .try_into()
            .map_err(|_| GuardError::AttestationFailed)?,
    );
    let revoked = data[attestation_offset::REVOKED] != 0;
    if revoked {
        return Ok(false);
    }
    if expires_at != 0 && Clock::get()?.unix_timestamp >= expires_at {
        return Ok(false);
    }

    Ok(true)
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

/// The amount field of an SPL token account lives at bytes 64..72 (u64 LE).
fn token_account_amount(account: &UncheckedAccount) -> Result<u64> {
    let data = account.try_borrow_data()?;
    let bytes: [u8; 8] = data
        .get(64..72)
        .and_then(|s| s.try_into().ok())
        .ok_or(GuardError::MetaListMismatch)?;
    Ok(u64::from_le_bytes(bytes))
}
