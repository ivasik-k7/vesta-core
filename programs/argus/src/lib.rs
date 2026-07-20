//! Argus — the hundred-eyed guard of VESTA point transfers.
//!
//! Implements the SPL transfer-hook interface: Token-2022 CPIs `execute` on
//! every transfer of a hooked mint. Policy (spec §4.3): permanent-delegate
//! flows pass (audited), payments to the merchant treasury pass, peer gifts
//! are velocity-capped per day, program-owned destinations are filtered.

pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;
use spl_discriminator::SplDiscriminate;
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx");

#[program]
pub mod argus {
    use super::*;

    /// Permissioned creation of the ExtraAccountMetaList (spec §4.1).
    pub fn initialize_transfer_guard(ctx: Context<InitializeTransferGuard>) -> Result<()> {
        instructions::initialize_transfer_guard::handle_initialize_transfer_guard(ctx)
    }

    /// One-time, customer-signed ledger creation — in-hook creation is
    /// impossible (privilege de-escalation leaves no rent payer).
    pub fn open_gift_ledger(ctx: Context<OpenGiftLedger>) -> Result<()> {
        instructions::open_gift_ledger::handle_open_gift_ledger(ctx)
    }

    /// Invoked by Token-2022 on every transfer of a hooked mint.
    #[instruction(discriminator = ExecuteInstruction::SPL_DISCRIMINATOR_SLICE)]
    pub fn execute(ctx: Context<Execute>, amount: u64) -> Result<()> {
        instructions::execute::handle_execute(ctx, amount)
    }
}
