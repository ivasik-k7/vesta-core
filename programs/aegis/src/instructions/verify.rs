use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    constants::{verify_reason, ATTESTATION_SEED, STATE_VERSION},
    error::AegisError,
    state::{attestation_status, Attestation},
};

/// The verdict a `verify` call returns (via `sol_set_return_data`). Read by the
/// caller with `sol_get_return_data`; never PII. Spec 07. `jurisdiction` / `tier`
/// let a composing verifier enforce scoped trust (0 = unset / not applicable).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, Default)]
pub struct Verdict {
    pub ok: bool,
    pub reason_code: u16,
    pub issuer: Pubkey,
    pub schema_id: u64,
    pub expires_at: i64,
    pub jurisdiction: u16,
    pub tier: u8,
}

/// The predicate `verify` evaluates (spec 07). Phase 1 ships `Present` only.
///
/// Selective disclosure (`AttributeDisclosed`) was intentionally removed: a
/// caller-supplied Merkle leaf with no value-binding or leaf/internal domain
/// separation is unsound (an empty path returns the leaf verbatim, and the
/// public `attr_root` could be replayed as a leaf). It returns in wave 2 with a
/// value-bound, domain-separated (or ZK) construction. More opcodes
/// (thresholds, accredited-by) are additive on this enum.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub enum VerifyPredicate {
    /// Subject holds a live (active, in-window) credential of `schema_id`
    /// issued by `issuer`.
    Present {
        issuer: Pubkey,
        subject: Pubkey,
        schema_id: u64,
    },
}

impl VerifyPredicate {
    fn target(&self) -> (Pubkey, Pubkey, u64) {
        match *self {
            VerifyPredicate::Present {
                issuer,
                subject,
                schema_id,
            } => (issuer, subject, schema_id),
        }
    }
}

#[derive(Accounts)]
pub struct Verify<'info> {
    /// CHECK: the aegis attestation PDA to evaluate. May be missing/empty — that
    /// yields a `NOT_FOUND` verdict, not an error. Re-derived and owner-checked
    /// against the predicate's (issuer, subject, schema_id) below.
    pub attestation: UncheckedAccount<'info>,
}

/// Stateless, read-only verdict primitive. Never reverts on a negative result —
/// it returns `Verdict { ok: false, reason_code }` so callers compose it.
pub fn handle_verify(ctx: Context<Verify>, predicate: VerifyPredicate) -> Result<()> {
    let (issuer, subject, schema_id) = predicate.target();
    let verdict = evaluate(&ctx.accounts.attestation, issuer, subject, schema_id, 0);
    emit_verdict(&verdict)
}

/// Serialize a `Verdict` into instruction return-data (shared by `verify` and
/// `verify_policy`).
pub(crate) fn emit_verdict(verdict: &Verdict) -> Result<()> {
    let mut buf = Vec::with_capacity(64);
    verdict
        .serialize(&mut buf)
        .map_err(|_| error!(AegisError::UnknownPredicate))?;
    set_return_data(&buf);
    Ok(())
}

fn fail(reason: u16) -> Verdict {
    Verdict {
        ok: false,
        reason_code: reason,
        ..Verdict::default()
    }
}

/// Evaluate a predicate over an attestation account, returning a `Verdict`.
/// `max_age_secs > 0` additionally requires the credential to have been issued
/// within that window (freshness / periodic re-verification — used by policies).
pub(crate) fn evaluate(
    account: &UncheckedAccount,
    issuer: Pubkey,
    subject: Pubkey,
    schema_id: u64,
    max_age_secs: i64,
) -> Verdict {
    // Pinned derivation: the account MUST be the canonical PDA for the predicate.
    let expected = Pubkey::find_program_address(
        &[
            ATTESTATION_SEED,
            issuer.as_ref(),
            subject.as_ref(),
            &schema_id.to_le_bytes(),
        ],
        &crate::ID,
    )
    .0;
    if account.key() != expected {
        return fail(verify_reason::NOT_FOUND);
    }
    // Missing / wrong-owner / empty → not found (fail closed).
    if account.owner != &crate::ID || account.data_is_empty() {
        return fail(verify_reason::NOT_FOUND);
    }
    let data = match account.try_borrow_data() {
        Ok(d) => d,
        Err(_) => return fail(verify_reason::NOT_FOUND),
    };
    let att = match Attestation::try_deserialize(&mut data.as_ref()) {
        Ok(a) => a,
        Err(_) => return fail(verify_reason::NOT_FOUND),
    };
    if att.version != STATE_VERSION {
        return fail(verify_reason::NOT_FOUND);
    }
    if att.issuer != issuer {
        return fail(verify_reason::WRONG_ISSUER);
    }
    if att.schema_id != schema_id {
        return fail(verify_reason::WRONG_SCHEMA);
    }
    if att.status != attestation_status::ACTIVE {
        return fail(verify_reason::NOT_ACTIVE);
    }
    let now = match Clock::get() {
        Ok(c) => c.unix_timestamp,
        Err(_) => return fail(verify_reason::NOT_ACTIVE),
    };
    if !att.is_live(now) {
        return fail(verify_reason::OUT_OF_WINDOW);
    }
    // Freshness / periodic re-verification (policy-driven).
    if max_age_secs > 0 && now.saturating_sub(att.issued_at) > max_age_secs {
        return fail(verify_reason::TOO_OLD);
    }

    Verdict {
        ok: true,
        reason_code: verify_reason::OK,
        issuer: att.issuer,
        schema_id: att.schema_id,
        expires_at: att.expires_at,
        ..Verdict::default()
    }
}
