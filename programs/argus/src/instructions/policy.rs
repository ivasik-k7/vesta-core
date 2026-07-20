use anchor_lang::prelude::*;

use crate::{constants::flags, error::GuardError, state::GuardConfig};

/// Full policy supplied at guard init. The attestation issuer/program are
/// fixed here for the life of the guard (baked into the meta list); everything
/// else is retunable via `configure_policy`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct InitialPolicy {
    pub flags: u16,
    pub daily_gift_cap: u64,
    pub per_tx_cap: u64,
    pub max_wallet_balance: u64,
    pub transfers_per_day_cap: u16,
    pub cooldown_secs: u32,
    /// aegis issuer to trust; `Pubkey::default()` when attestation is unused.
    pub attestation_issuer: Pubkey,
    pub attestation_schema: u16,
    pub attestation_mask: u64,
}

/// Partial retune. Every field is optional; `None` leaves the value untouched.
/// The attestation issuer is intentionally absent — it is immutable post-init.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, Default)]
pub struct PolicyUpdate {
    pub flags: Option<u16>,
    pub daily_gift_cap: Option<u64>,
    pub per_tx_cap: Option<u64>,
    pub max_wallet_balance: Option<u64>,
    pub transfers_per_day_cap: Option<u16>,
    pub cooldown_secs: Option<u32>,
    pub attestation_schema: Option<u16>,
    pub attestation_mask: Option<u64>,
}

impl PolicyUpdate {
    /// Apply the set fields onto `config`, then validate the result as a whole.
    pub fn apply(&self, config: &mut GuardConfig) -> Result<()> {
        if let Some(v) = self.flags {
            config.flags = v;
        }
        if let Some(v) = self.daily_gift_cap {
            config.daily_gift_cap = v;
        }
        if let Some(v) = self.per_tx_cap {
            config.per_tx_cap = v;
        }
        if let Some(v) = self.max_wallet_balance {
            config.max_wallet_balance = v;
        }
        if let Some(v) = self.transfers_per_day_cap {
            config.transfers_per_day_cap = v;
        }
        if let Some(v) = self.cooldown_secs {
            config.cooldown_secs = v;
        }
        if let Some(v) = self.attestation_schema {
            config.attestation_schema = v;
        }
        if let Some(v) = self.attestation_mask {
            config.attestation_mask = v;
        }
        validate_policy(
            config.flags,
            config.daily_gift_cap,
            config.per_tx_cap,
            config.attestation_issuer,
        )
    }
}

/// The guard authority is the merchant wallet bound at init (verified there via
/// the full vesta_core Merchant chain). Thereafter it is a plain signer, so it
/// can be rotated two-step to any wallet independently of vesta_core.
pub fn require_guard_authority(config_authority: Pubkey, signer: Pubkey) -> Result<()> {
    require_keys_eq!(config_authority, signer, GuardError::Unauthorized);
    Ok(())
}

/// Coherence checks shared by init and configure (spec §3.2).
pub fn validate_policy(
    flags: u16,
    daily_gift_cap: u64,
    per_tx_cap: u64,
    attestation_issuer: Pubkey,
) -> Result<()> {
    require!(flags & !flags::KNOWN == 0, GuardError::UnknownFlag);
    // A per-tx cap above the daily cap is meaningless — reject the confusion.
    if daily_gift_cap != 0 && per_tx_cap != 0 {
        require!(per_tx_cap <= daily_gift_cap, GuardError::InvalidPolicy);
    }
    // Requiring attestation without naming an issuer can never pass — fail loud.
    if flags & flags::REQUIRE_ATTESTATION != 0 {
        require!(
            attestation_issuer != Pubkey::default(),
            GuardError::InvalidPolicy
        );
    }
    Ok(())
}
