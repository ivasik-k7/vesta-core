use anchor_lang::prelude::*;

use crate::{
    constants::{MAX_STANDARD_URI_LEN, SCHEMA_SEED, STATE_VERSION},
    error::AegisError,
    events::{SchemaDeprecated as SchemaDeprecatedEvent, SchemaRegistered},
    state::Schema,
};

/// Register a typed, versioned credential schema. Schemas are shapes, not
/// instances — no subject data. A subject's credentials reference `id` and
/// consumers interpret disclosed attributes against this shape.
#[derive(Accounts)]
#[instruction(id: u64)]
pub struct RegisterSchema<'info> {
    #[account(mut)]
    pub registrar: Signer<'info>,

    #[account(
        init,
        payer = registrar,
        space = 8 + Schema::INIT_SPACE,
        seeds = [SCHEMA_SEED, registrar.key().as_ref(), &id.to_le_bytes()],
        bump,
    )]
    pub schema: Account<'info, Schema>,

    pub system_program: Program<'info, System>,
}

pub fn handle_register_schema(
    ctx: Context<RegisterSchema>,
    id: u64,
    content_hash: [u8; 32],
    standard_uri: String,
    sas_schema: Option<Pubkey>,
) -> Result<()> {
    require!(
        standard_uri.len() <= MAX_STANDARD_URI_LEN,
        AegisError::StringTooLong
    );
    let schema = &mut ctx.accounts.schema;
    schema.version = STATE_VERSION;
    schema.registrar = ctx.accounts.registrar.key();
    schema.id = id;
    schema.content_hash = content_hash;
    schema.standard_uri = standard_uri;
    schema.sas_schema = sas_schema;
    schema.deprecated = false;
    schema.successor = None;
    schema.bump = ctx.bumps.schema;

    emit!(SchemaRegistered {
        schema: schema.key(),
        registrar: schema.registrar,
        id,
    });
    Ok(())
}

/// Mark a schema deprecated (registrar only). Existing attestations under it
/// remain valid until expiry/revocation; new issuance should move to `successor`.
#[derive(Accounts)]
pub struct DeprecateSchema<'info> {
    pub registrar: Signer<'info>,

    #[account(
        mut,
        has_one = registrar @ AegisError::Unauthorized,
        seeds = [SCHEMA_SEED, registrar.key().as_ref(), &schema.id.to_le_bytes()],
        bump = schema.bump,
    )]
    pub schema: Account<'info, Schema>,
}

pub fn handle_deprecate_schema(
    ctx: Context<DeprecateSchema>,
    successor: Option<Pubkey>,
) -> Result<()> {
    let schema = &mut ctx.accounts.schema;
    schema.deprecated = true;
    schema.successor = successor;
    emit!(SchemaDeprecatedEvent {
        schema: schema.key(),
        successor,
    });
    Ok(())
}
