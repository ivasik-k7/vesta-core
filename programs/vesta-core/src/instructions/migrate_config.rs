use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

use crate::{constants::CONFIG_SEED, error::VestaError, events::ConfigMigrated, state::Config};

/// v1 layout: discriminator(8) + admin(32) + paused(1) + bump(1)
const V1_LEN: usize = 8 + 32 + 1 + 1;
const V2_LEN: usize = 8 + Config::INIT_SPACE;

/// One-shot in-place migration of the deployed v1 Config (no `pending_admin`)
/// to the v4-spec layout, preserving the program id and every published link.
#[derive(Accounts)]
pub struct MigrateConfig<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// CHECK: v1 layout cannot deserialize as `Config`; verified manually below
    /// (PDA seeds, discriminator, stored admin, and v1 length).
    #[account(mut, seeds = [CONFIG_SEED], bump)]
    pub config: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_migrate_config(ctx: Context<MigrateConfig>) -> Result<()> {
    let config_info = ctx.accounts.config.to_account_info();

    require!(
        config_info.data_len() == V1_LEN,
        VestaError::MigrationAlreadyApplied
    );

    let (admin, paused, bump) = {
        let data = config_info.try_borrow_data()?;
        require!(
            data[..8] == Config::DISCRIMINATOR[..],
            VestaError::Unauthorized
        );
        let admin = Pubkey::try_from(&data[8..40]).map_err(|_| VestaError::Unauthorized)?;
        (admin, data[40] != 0, data[41])
    };
    require_keys_eq!(admin, ctx.accounts.admin.key(), VestaError::Unauthorized);

    // Top up rent for the new size, then realloc and rewrite in the v2 layout.
    let rent = Rent::get()?;
    let needed = rent
        .minimum_balance(V2_LEN)
        .saturating_sub(config_info.lamports());
    if needed > 0 {
        transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.admin.to_account_info(),
                    to: config_info.clone(),
                },
            ),
            needed,
        )?;
    }
    config_info.resize(V2_LEN)?;

    let migrated = Config {
        admin,
        pending_admin: None,
        paused,
        bump,
    };
    let mut data = config_info.try_borrow_mut_data()?;
    let mut cursor: &mut [u8] = &mut data;
    migrated.try_serialize(&mut cursor)?;

    emit!(ConfigMigrated { admin });
    Ok(())
}
