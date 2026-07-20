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
pub struct AttestationIssued {
    pub issuer: Pubkey,
    pub subject: Pubkey,
    pub schema: u16,
    pub value: u64,
    pub expires_at: i64,
}

#[event]
pub struct AttestationUpdated {
    pub issuer: Pubkey,
    pub subject: Pubkey,
    pub schema: u16,
    pub value: u64,
    pub expires_at: i64,
}

#[event]
pub struct AttestationRevoked {
    pub issuer: Pubkey,
    pub subject: Pubkey,
}
