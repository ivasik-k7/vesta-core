# 11 · vesta_core — Accredited Merchant Identity & Solvency

> **Status:** ✅ Implemented (vesta_core v2.1.0) · **Track:** D (Merchant) · **Layer:** Foundation / regulated-operator trust · **Codename:** NOMOS · **Depends on:** 08 (aegis accreditation), argus trust-triangle (shipped v2.1.0)
> Inherits all [shared conventions](README.md#shared-conventions-normative-for-all-specs).
>
> **Implemented:** (§4.1) `MerchantTrust` + permissionless `reverify_merchant` crank
> (CPIs aegis `verify_accreditation`, pins the aegis PDAs from the anchor — no
> griefing) + grace-window auto-degrade / auto-restore of `Merchant.issue_status`;
> `set_merchant_trust` + `set_merchant_issue_status` (manual override); the earn
> gate (`earn_points`/`earn_points_campaign` require `issue_status == NORMAL`;
> redemption + clawback never gated). (§4.2) `MerchantReserve` +
> `open`/`fund`/`withdraw`(coverage-enforced)/`attest_reserve` proof-of-reserves,
> liability measured on raw supply (decay-conservative).
>
> **Deferred (documented):** the *pre-mint* solvency gate on `earn` (§4.2) — it
> would thread optional reserve accounts through the earn hot path and every earn
> call site; withdrawal-coverage + public attestation already guarantee reserves
> cannot be pulled below outstanding liability and that under-collateralization is
> provable. `verified` is retained as the legacy admin badge (not removed);
> `issue_status` is the real signal.

## 1. Summary

Promote the merchant from a **cosmetic, admin-set `verified` boolean** into an
**accredited, revocable, examinable issuer** whose *authority to mint points*
derives from an aegis accreditation root and evaporates automatically when that
accreditation is pulled — and back the liability it mints with an on-chain
reserve. Two capabilities, one doctrine:

- **Accredited identity (§4.1).** A `MerchantTrust` account binds the merchant to
  an aegis accreditation root (spec 08); a permissionless `reverify_merchant`
  crank re-checks the chain via aegis `verify_accreditation` and, after a grace
  window, trips the merchant into a degraded posture. This is a near-verbatim
  port of the shipped argus `TrustAnchor` + `reverify_accreditation` state
  machine — applied to *issuance* instead of transfers.
- **Reserve-backed solvency (§4.2).** A `MerchantReserve` escrows a caller-chosen
  stablecoin against outstanding point liability, so base-earn issuance can be
  required to stay solvent — the liability stream that spec 01 (Funded Campaign
  Vaults) explicitly leaves unbacked.

**Governing doctrine (inherited from argus `trust.rs`):** *a compliance or
solvency failure freezes **issuance**, never **redemption**.* A revoked or
insolvent merchant can mint no new liability, but every existing holder can still
redeem and clawback stays open — assets are never stranded.

## 2. Motivation & current gap

- **`Merchant.verified` is theater.** It is a `bool` set by `handle_verify_merchant`
  gated only on `config.admin` — no jurisdiction, no tier, no expiry, no trust
  root, no revocation propagation. A mainnet loyalty issuer (or a bank running a
  points program) cannot stand behind it.
- **Issuance is unbacked.** `earn_points` / `earn_points_campaign` mint freely
  with the merchant PDA as mint authority; `lifetime_points_issued` is a bare
  counter. Nothing on-chain guarantees the merchant can honour redemptions. Spec
  01 escrows *campaign bonus* budgets but states base earn "mints as today" —
  the largest liability stream stays unbacked even after 01 ships.
- **The trust machinery already exists next door and is unused here.** argus
  gives the merchant's *mint* a full trust triangle (`TrustAnchor` +
  `reverify_accreditation` auto-degrade) — but that governs *peer transfers*, not
  the merchant's authority to *issue*. aegis `verify_accreditation` is a shipped,
  return-data verdict primitive. This spec wires them into the economy.

## 3. Goals / Non-goals

**Goals**
- Replace the `verified` bool with a real, revocable, jurisdiction- and
  tier-bearing merchant identity that chains to an aegis accreditation root.
- Auto-degrade issuance on revocation via a permissionless crank + grace window;
  auto-restore on recovery — no human key in the loop.
- Optional reserve backing for issued liability, with an examiner-facing reserve
  attestation and a solvency gate on earn.
- Strictly additive and opt-in: a merchant that adopts neither behaves exactly as
  today.

**Non-goals**
- Internal role separation / RBAC (→ spec 13) and customer-side identity gating
  (→ spec 12).
- aegis internals (06–08); this spec *consumes* `verify_accreditation`.
- Per-campaign bonus escrow (owned by spec 01); §4.2 is the merchant-wide
  base-earn reserve that composes above spec 01's campaign vaults.

## 4. Design

### 4.1 Accredited merchant identity + auto-degrade

`MerchantTrust` `["mtrust", merchant]` is the merchant-side analogue of argus's
`TrustAnchor`:

```
MerchantTrust
  version            : u8
  merchant           : Pubkey
  accreditation_root : Pubkey   // aegis TrustRoot identity the merchant chains to
  subject_issuer     : Pubkey   // the merchant's aegis KYB subject/issuer identity
  required_schema    : u64      // the accreditation "type" required
  aegis_program      : Pubkey
  degrade_target     : u8       // EARN_FROZEN | REDEMPTION_ONLY (never NORMAL)
  grace_secs         : i64      // tolerance for a transient aegis outage
  failing_since      : i64      // 0 = healthy
  last_verified_at   : i64
  tier               : u8       // captured from the accreditation verdict
  jurisdiction       : u16      // "
  bump               : u8
```

The live posture is denormalized onto the merchant as `Merchant.issue_status: u8`
(appended past the ABI prefix — see §8) so the earn hot path reads it for free.

- `set_merchant_trust(root, subject_issuer, schema, degrade_target, grace)` —
  owner-only; mirrors argus `handle_set_trust_anchor`. Resets health to a clean
  baseline.
- `reverify_merchant(...)` — **permissionless crank**. Pins the aegis TrustRoot /
  Accreditation PDAs from `(accreditation_root, subject_issuer)` (invariant #3 —
  identical to the argus hardening), CPIs aegis `verify_accreditation`, reads the
  `Verdict` from return-data, and runs the same grace-window streak logic:
  healthy ⇒ `issue_status = NORMAL`, clear streak; failing past `grace_secs` ⇒
  `issue_status = degrade_target`. Auto-restores on the next healthy crank.
- **Earn gate:** `earn_points` / `earn_points_campaign` add
  `require!(merchant.issue_status == NORMAL, IssuanceFrozen)`. Redemption and
  clawback deliberately **do not** check it.
- **`verified` becomes derived:** keep the field (append-safe), stop
  admin-writing it, and let the crank set it as a cache of `issue_status ==
  NORMAL`. `handle_verify_merchant` is deprecated (kept as a no-op alias during
  migration, then removed).

```mermaid
sequenceDiagram
    autonumber
    actor X as anyone (crank)
    participant VC as vesta_core
    participant AE as aegis
    X->>VC: reverify_merchant(merchant)
    VC->>AE: verify_accreditation(root, subject_issuer, schema) (CPI)
    AE-->>VC: Verdict via return-data (never reverts)
    alt healthy
        VC->>VC: issue_status = NORMAL · clear streak
    else failing past grace
        VC->>VC: issue_status = degrade_target (EARN_FROZEN)
    end
    Note over VC: earn_points requires issue_status == NORMAL;<br/>redemption & clawback ignore it — holders never stranded
```

### 4.2 Point-liability reserve & solvency

`MerchantReserve` `["mreserve", merchant]`:

```
MerchantReserve
  version        : u8
  merchant       : Pubkey
  backing_mint   : Pubkey   // caller-supplied SPL/Token-2022 stablecoin
  reserve_ata    : Pubkey   // PDA-owned escrow token account (authority = this PDA)
  unit_value     : u64      // stable minor units per 1 UI point (governance-set; no oracle)
  target_ratio_bps : u16    // fractional reserve (breakage-adjusted); 10_000 = full
  total_backed   : u128
  bump           : u8
```

- `open_reserve(backing_mint, unit_value, target_ratio_bps)` — owner-only; creates
  the escrow ATA under the PDA.
- `fund_reserve(amount)` / `withdraw_reserve(amount)` — owner-only; a withdrawal
  may not drop coverage below outstanding liability × `target_ratio_bps`.
- `attest_reserve()` — permissionless; emits `ReserveAttested { outstanding_ui,
  reserve_stable, ratio_bps, ts }` for examiners.
- **Solvency gate on earn (opt-in):** when a reserve exists and the merchant sets
  `REQUIRE_RESERVE`, `earn_points` requires
  `reserve_balance ≥ ui_to_stable(outstanding + minted) × target_ratio_bps / 10_000`,
  where `outstanding = lifetime_points_issued − redeemed_raw − clawed_back` in UI
  units (via the shared `amount_to_ui_amount` path). Accounting-only — the stable
  stays escrowed and is consumed at settlement/breakage, exactly as spec 01's
  `StableReserve` arithmetic.

Composes with spec 01: campaign-bonus vaults (01) nest *under* the merchant-wide
base-earn reserve here; no duplication.

## 5. Account model (new)

```
MerchantTrust     seeds = ["mtrust", merchant]      // §4.1 accreditation anchor
MerchantReserve   seeds = ["mreserve", merchant]    // §4.2 liability escrow
Merchant (appended, past the 112-byte ABI prefix)
  + issue_status  : u8    // NORMAL | EARN_FROZEN | REDEMPTION_ONLY
  + reserve_flags : u8    // REQUIRE_RESERVE, …
```

## 6. Instruction surface (new)

- Identity: `set_merchant_trust`, `reverify_merchant` (crank), `set_merchant_issue_status`
  (owner manual override / dispute restore, mirrors argus `set_degrade_mode`).
- Reserve: `open_reserve`, `fund_reserve`, `withdraw_reserve`, `attest_reserve`.
- Gates woven into existing `earn_points` / `earn_points_campaign` (issuance +
  optional solvency); `redeem_offer` / `clawback` unchanged (never gated).

## 7. Math & limits

- Grace/streak/epoch comparisons and all reserve arithmetic use `checked_*`.
- `unit_value` is governance-set (no oracle) exactly per spec 01 §12; cross-mint
  UI↔raw conversion goes through the shared `amount_to_ui_amount` path.
- Outstanding liability is monotone-safe: redemptions/clawbacks are already
  counted on `Merchant`; the gate uses `saturating_sub` so a rounding wobble can
  never underflow into "infinite headroom".

## 8. Security & compatibility

- **Merchant ABI prefix untouched.** New scalars (`issue_status`, `reserve_flags`)
  **append after `mint_bump`**, well past the argus-read prefix
  (`disc·id·authority·point_mint·treasury`, first 112 bytes). `MerchantTrust` /
  `MerchantReserve` are new PDAs — zero ABI impact.
- **Degrade scoped to issuance only.** A tested invariant: `redeem_offer` and
  `clawback` must ignore `issue_status`. Freezing them would strand holders — the
  exact failure argus's design avoids.
- **Pinned cross-program derivation (#3):** the crank re-derives the aegis PDAs
  from the anchor's own root/subject (never caller-supplied) so a permissionless
  caller cannot force a false verdict — the same hardening applied to argus's
  `reverify_accreditation`.
- **Reserve custody:** the escrow ATA authority is the `MerchantReserve` PDA;
  withdrawal is owner-only and coverage-checked; `attest_reserve` is
  permissionless (read-only emit).
- Appending fields grows `Merchant::INIT_SPACE` → realloc-on-touch or fresh
  deploy (devnet disposable); **do not remove** `verified` (repurpose it) or any
  indexer keyed on enterprise-region offsets breaks.

## 9. Test plan (LiteSVM)

- `reverify_merchant` healthy ⇒ `issue_status = NORMAL`; revoke accreditation ⇒
  after grace, `EARN_FROZEN`; grace window absorbs a transient failure; recovery
  auto-restores.
- Earn blocked under `EARN_FROZEN`; **redeem + clawback still succeed** (the
  no-stranding invariant).
- Permissionless crank with wrong aegis accounts is rejected (pinned derivation).
- Reserve: fund/withdraw with coverage floor; earn blocked when reserve below the
  required ratio; `attest_reserve` emits correct outstanding/ratio.

## 10. Phased rollout

1. **Accredited identity + auto-degrade** — `MerchantTrust`, crank, earn gate,
   `verified` deprecation. *(The keystone; a near-verbatim argus port.)*
2. **Reserve & solvency** — `MerchantReserve`, fund/withdraw/attest, opt-in earn
   solvency gate.
3. Manual override + examiner reporting polish; fold campaign-vault (spec 01)
   reserves into the merchant reserve report.

## 11. Open questions

- Should accreditation be **required** for any issuance on mainnet (hard gate) or
  remain opt-in with `verified` as the signal? (Recommend: opt-in on devnet,
  required-per-jurisdiction on mainnet.)
- Reserve `unit_value` governance cadence — static vs. governed re-price (ties to
  spec 13's governed config lifecycle).
- Whether `REDEMPTION_ONLY` and `EARN_FROZEN` differ in this economy, or collapse
  to one "issuance frozen" posture (argus faced the same question).
