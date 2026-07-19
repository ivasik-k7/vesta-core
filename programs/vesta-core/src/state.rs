use anchor_lang::prelude::*;

/// Global protocol config. Singleton PDA, created once at deployment.
#[account]
#[derive(InitSpace)]
pub struct Config {
    pub admin: Pubkey,
    pub paused: bool,
    pub bump: u8,
}
