pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("Am2X4B1SCnJKXL8Yir2j6yGpHAKrmwcf2E5aKnA9BZV");

#[program]
pub mod vesta_core {
    use super::*;

    pub fn init_config(ctx: Context<InitConfig>) -> Result<()> {
        instructions::init_config::handle_init_config(ctx)
    }
}
