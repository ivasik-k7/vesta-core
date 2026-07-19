//! Argus — the hundred-eyed guard of VESTA point transfers.
//!
//! Phase 2 will implement the SPL transfer hook interface here:
//! `Execute` validation (whitelisted flows, daily gift limits) and
//! `InitializeExtraAccountMetaList`. Placeholder instruction for now
//! so the workspace builds and deploys end-to-end.

use anchor_lang::prelude::*;

declare_id!("CrzLCMSQ1pWTuLXBomoLn6eAB1c1gLsw5x9sBeuyBNKt");

#[program]
pub mod argus {
    use super::*;

    pub fn ping(_ctx: Context<Ping>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Ping {}
