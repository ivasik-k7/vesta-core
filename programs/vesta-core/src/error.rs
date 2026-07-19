use anchor_lang::prelude::*;

#[error_code]
pub enum VestaError {
    #[msg("Protocol is paused")]
    ProtocolPaused,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Arithmetic overflow")]
    Overflow,
}
