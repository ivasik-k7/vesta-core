use anchor_lang::prelude::*;

#[event]
pub struct IssuerInitialized {
    pub issuer: Pubkey,
    pub authority: Pubkey,
}

#[event]
pub struct IssuerPausedSet {
    pub issuer: Pubkey,
    pub paused: bool,
}

#[event]
pub struct IssuerOperatorSet {
    pub issuer: Pubkey,
    pub operator: Option<Pubkey>,
}

#[event]
pub struct IssuerAuthorityProposed {
    pub issuer: Pubkey,
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct IssuerAuthorityChanged {
    pub issuer: Pubkey,
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct SchemaRegistered {
    pub schema: Pubkey,
    pub registrar: Pubkey,
    pub id: u64,
}

#[event]
pub struct SchemaDeprecated {
    pub schema: Pubkey,
    pub successor: Option<Pubkey>,
}

#[event]
pub struct AttestationIssued {
    pub issuer: Pubkey,
    pub subject: Pubkey,
    pub schema_id: u64,
    pub valid_from: i64,
    pub expires_at: i64,
}

#[event]
pub struct AttestationUpdated {
    pub issuer: Pubkey,
    pub subject: Pubkey,
    pub schema_id: u64,
    pub valid_from: i64,
    pub expires_at: i64,
}

#[event]
pub struct AttestationRevoked {
    pub issuer: Pubkey,
    pub subject: Pubkey,
    pub schema_id: u64,
    pub reason_code: u16,
}

#[event]
pub struct AttestationErased {
    pub issuer: Pubkey,
    pub subject: Pubkey,
    pub schema_id: u64,
}

#[event]
pub struct AttestationClosed {
    pub issuer: Pubkey,
    pub subject: Pubkey,
    pub schema_id: u64,
}
