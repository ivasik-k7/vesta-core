use anchor_lang::{
    prelude::*,
    solana_program::program::{get_return_data, invoke},
};

use crate::error::VestaError;

/// Integer-only "12.34" formatting for a UI-points value carrying two decimals.
pub(crate) fn format_ui_amount(ui_points: u64) -> String {
    format!("{}.{:02}", ui_points / 100, ui_points % 100)
}

/// UI → raw via Token-2022's UiAmountToAmount (u64 LE return data). One
/// direction only — float conversions are documented as non-round-trippable;
/// caller-supplied slippage bounds absorb the residual wobble (spec §3.4).
pub(crate) fn ui_points_to_raw<'info>(
    token_program: &Pubkey,
    mint: &AccountInfo<'info>,
    ui_points: u64,
) -> Result<u64> {
    let ui_str = format_ui_amount(ui_points);
    let ix = spl_token_2022_interface::instruction::ui_amount_to_amount(
        token_program,
        mint.key,
        &ui_str,
    )
    .map_err(|_| VestaError::ConversionFailed)?;
    invoke(&ix, std::slice::from_ref(mint))?;
    let (returner, data) = get_return_data().ok_or(VestaError::ConversionFailed)?;
    require_keys_eq!(returner, *token_program, VestaError::ConversionFailed);
    let raw = u64::from_le_bytes(
        data.as_slice()
            .try_into()
            .map_err(|_| VestaError::ConversionFailed)?,
    );
    Ok(raw)
}
