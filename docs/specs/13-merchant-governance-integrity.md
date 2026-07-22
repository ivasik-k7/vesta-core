# 13 · vesta_core — Merchant Governance & Operational Integrity

> **Status:** ◒ Core shipped (vesta_core v2.1.0) · **Track:** D (Merchant) · **Layer:** Integrity / separation-of-duties / audit · **Codename:** EPHORATE · **Depends on:** argus governance model (shipped v2.1.0); (11) for trust status
> Inherits all [shared conventions](README.md#shared-conventions-normative-for-all-specs).
>
> **Implemented:** (§4.1) operator RBAC / separation of duties — `Merchant`
> `governance_enabled` + `cashier` / `campaign_manager` role keys, gating the
> mint path and campaign/offer/badge creation (`may_earn` / `may_manage`), set via
> `set_merchant_governance`. (§4.2) issuance circuit breaker —
> `Merchant.daily_issue_cap_raw` enforced in `accrue` (symmetric to the clawback
> cap), set via `set_daily_issue_cap`. (§4.4) decision statements —
> `MerchantStatement` + `anchor_merchant_statement` (owner-anchored Merkle root +
> completeness witness).
>
> **Backlog (documented, not built):** the governed config lifecycle (§4.3) — a
> large port of argus `PolicyVersion`/`PolicyPointer`; the shipped RBAC + issuance
> cap already bound the concrete insider-risk it targets. Dual-control clawback +
> owner recovery (§4.5) — deferred for stack-frame risk on the already-tight
> clawback instruction relative to marginal value over the shipped owner-only +
> daily-cap controls. Role management is owner-set (timelocked role changes not
> yet ported).

## 1. Summary

Port argus's shipped enterprise-governance model into the merchant economy — the
place where value is actually minted — so that **no single compromised key can
drain issuance** and **every privileged action is board- and auditor-provable**.
Four capabilities:

- **Scoped operator RBAC + separation of duties (§4.1)** — replace the flat
  all-or-nothing operator set with roles (Cashier / CampaignManager / Treasurer /
  Compliance / Reporter / Configurator / Approver / PauseOperator / RoleAdmin),
  each a key or multisig, gated fail-closed.
- **Issuance circuit breaker (§4.2)** — per-operator and per-merchant daily mint
  caps + an over-issuance auto-freeze; the far-larger sibling of the clawback
  daily cap that already exists.
- **Governed config lifecycle (§4.3)** — propose → approve (≠ author) → timelock →
  activate → rollback → pin for high-impact config (earn rate, treasury re-point,
  clawback cap, issuance limits, role grants) — replacing today's silent
  single-signer live-mutation.
- **Merchant decision statements (§4.4)** — a tamper-evident, provably-complete,
  reason-coded ledger of earns / redemptions / clawbacks / config changes.

Every piece is a near-verbatim port of a shipped argus primitive
(`RoleRegistry`, `PolicyVersion`/`PolicyPointer`, `StatementCommitment`), applied
to merchant operations. Governance is **opt-in and additive**: a merchant that
never adopts it keeps today's flat behavior.

## 2. Motivation & current gap

- **Flat authorization — one privilege for five powers.** `Merchant::can_operate`
  is true for the owner *or any* of ≤4 operators, with no scoping. That single
  predicate gates minting points (`earn_points`), creating campaigns
  (`create_campaign`), creating offers, and minting badge NFTs. A single
  compromised POS key can do all of it.
- **Unbounded issuance from a hot key.** `accrue` enforces only a *per-transaction*
  `MAX_EARN_PER_TX`; there is no per-operator/per-day/per-merchant issuance
  ceiling. Clawback got a daily cap precisely for compromised-key defense — the
  far larger surface, *issuance*, has none.
- **A concrete rogue-operator path.** Campaign *create* is operator-allowed but
  campaign *update* is owner-only — so an operator can create a campaign with a
  huge budget and max multiplier, then drain it through `earn_points_campaign`.
- **Silent live-mutation.** Every owner control (`set_merchant_operator`,
  `set_clawback_cap`, `set_merchant_paused`, `update_merchant` earn-rate re-tune)
  is an instant single-signer write with only an event — no proposal, approval,
  timelock, diff, or rollback. argus *explicitly replaced this exact pattern.*
- **No decision statements.** vesta_core emits events but has no provably-complete,
  Merkle-anchored ledger — while argus has exactly that for transfer decisions.

## 3. Goals / Non-goals

**Goals**
- Least-privilege operator roles with fail-closed checks and timelocked grants.
- A daily issuance blast-radius cap + over-issuance auto-freeze (freeze-only —
  redemption/clawback stay open).
- Governed lifecycle for high-impact merchant config; instant freeze-only stays.
- A tamper-evident, reason-coded, provably-complete merchant decision ledger.
- Dual-control clawback above a threshold + a recovery path that reintroduces no
  god key. Additive and opt-in throughout.

**Non-goals**
- Merchant-side accreditation (→ spec 11), customer identity gating (→ spec 12).
- Re-deriving the governance state machine — it is ported from argus, not
  reinvented.

## 4. Design

### 4.1 `MerchantRoles` — scoped RBAC + SoD

`MerchantRoles` `["mroles", merchant]` mirrors argus `RoleRegistry` (same
fail-closed `require()`; each role a key or multisig). Role → gated instructions:

| Role | Gates |
|---|---|
| `Cashier` | `earn_points`, `earn_points_campaign` (the mint path) |
| `CampaignManager` | `create/update_campaign`, `create_offer`, achievements — *create and update as one role* (closes the create/update asymmetry) |
| `Treasurer` | reserve fund/withdraw (spec 11), treasury operations |
| `Compliance` | clawback co-signer, screening controls |
| `Reporter` | `anchor_merchant_statement` (§4.4) |
| `Configurator` / `Approver` | propose / approve config (§4.3; approver ≠ author) |
| `PauseOperator` | instant freeze-only |
| `RoleAdmin` | grants/revokes roles via a **two-step timelocked** change |

`can_operate` becomes: *if a `MerchantRoles` PDA exists, resolve the specific
role; else fall back to `operators[4]`* — fully additive, opt-in. Role changes
reuse argus `propose_role_change` / `apply_role_change` verbatim.

### 4.2 Issuance circuit breaker

Per-operator + per-merchant daily mint caps, enforced in `accrue` using the
exact day-rollover pattern already proven for clawback (`clawed_today` /
`clawback_day`). State lives on an `IssuanceControl` PDA (or folded into
`MerchantRoles`): `per_operator_daily_raw`, `per_merchant_daily_raw`,
`minted_today`, `day`. On breach ⇒ auto-trip **freeze-only** (`issue_status =
EARN_FROZEN`, shared with spec 11) — minting blocked; redemption and clawback stay
open, never stranding holders. `set_issuance_limits` is governed via §4.3.

### 4.3 Governed merchant-config lifecycle

`MerchantConfigVersion` `["mcfg", merchant, hash]` (content-addressed, immutable)
+ `MerchantConfigPointer` `["mcfgptr", merchant]` (active/pending + `timelock_secs`
+ `pinned`) — a near-verbatim port of argus `PolicyVersion`/`PolicyPointer` and its
propose → approve(≠author) → timelock → activate → rollback → pin lifecycle.

- **Timelocked (governed):** `base_earn_rate`, decay rate, `treasury` re-point,
  `clawback_daily_cap_raw`, issuance limits, role grants.
- **Instant (PauseOperator only):** freeze/pause. Freeze must never be timelocked;
  unfreeze must be governed.

Adopting governance flips a `Merchant.governed` flag (appended) that, when set,
routes the instant-write admin instructions (`set_clawback_cap`, `update_merchant`
earn-rate, `set_merchant_operator`) through the lifecycle instead — exactly how
argus's `governed` flag disables `configure_policy`.

### 4.4 Merchant decision statements

Enrich `PointsEarned` / `CampaignBonusPaid` / redemption / `ClawbackEvent` /
config-change events with a canonical `decision_reason` code + a deciding-context
stamp (issuance headroom, trust status, gate result). A `MerchantStatement`
`["mstmt", merchant, period]` anchors a period's Merkle root + `decision_count`
completeness witness, anchored by the `Reporter` role — a direct analogue of
argus `StatementCommitment` + `anchor_statement`. One governance substrate, two
statement families: transfer decisions (argus) and economic decisions (here).

### 4.5 Dual-control clawback + recovery

- **Dual control:** clawback above a governed threshold requires owner **and**
  `Compliance` (two signers, or two-step propose/execute), on top of the existing
  owner-only gate + daily cap.
- **Recovery:** a `Guardian`-initiated, *timelocked* owner-authority rotation.
  Because `authority` is a Merchant PDA seed, rotation is a **re-key ceremony**
  (new PDA, migrate, close old) — **not** an in-place field write; v1 may instead
  model recovery as a governance-controlled operator reset (see §8).

## 5. Account model (new)

```
MerchantRoles          seeds = ["mroles", merchant]           // §4.1 RBAC
IssuanceControl        seeds = ["issctl", merchant]           // §4.2 (or fold into MerchantRoles)
OperatorMeter          seeds = ["ometer", merchant, operator] // §4.2 per-operator daily meter
MerchantConfigVersion  seeds = ["mcfg", merchant, hash]       // §4.3 immutable
MerchantConfigPointer  seeds = ["mcfgptr", merchant]          // §4.3 active/pending + timelock
MerchantStatement      seeds = ["mstmt", merchant, period]    // §4.4 audit anchor
Merchant (appended, past the 112-byte ABI prefix)
  + governed : bool
```

## 6. Instruction surface (new)

- Roles: `initialize_merchant_governance`, `propose_merchant_role_change`,
  `apply_merchant_role_change`.
- Issuance: `set_issuance_limits` (governed).
- Config lifecycle: `propose_merchant_config`, `approve_merchant_config`,
  `activate_merchant_config`, `rollback_merchant_config`, `pin_merchant_config`.
- Audit: `anchor_merchant_statement` (Reporter).
- Clawback/recovery: dual-control `clawback` path, `propose/accept_merchant_authority`.
- Existing gated instructions (`earn_points`, `create_campaign`, `create_offer`,
  achievements, `clawback`, `set_clawback_cap`, `update_merchant`) consult
  `MerchantRoles` / the lifecycle when governance is adopted.

## 7. Math & limits

- Daily meters use the proven `checked_*` day-rollover pattern (`day` vs.
  `unix_day`); a breach compares `minted_today + minted > cap`.
- Timelock/epoch comparisons `checked_*`; role registry fail-closed (unset role
  authority matches nothing).
- Bounded config docs; content hash = sha256 over borsh (as argus `PolicyDoc`).

## 8. Security & compatibility

- **Merchant ABI prefix untouched.** All governance state is in **new PDAs**; the
  only Merchant scalar added (`governed`) **appends past `mint_bump`** — outside
  the argus-read 112-byte prefix.
- **`operators[4]` kept, not removed.** It sits in the mutable tail (not the argus
  prefix), but removing it changes `INIT_SPACE` → migration. Instead deprecate it
  in place; `can_operate` consults `MerchantRoles` when present and falls back
  otherwise — additive, opt-in, zero migration (the argus `governed` pattern).
- **Freeze scoped to issuance.** The over-issuance auto-freeze and any degrade
  affect minting only; redemption/clawback stay open (shared invariant with
  spec 11 — never strand holders).
- **The one breaking watch-out:** §4.5 owner rotation. `authority` is a PDA seed,
  so a literal swap is a re-key migration, not a field write — scope it carefully
  or ship the operator-reset form in v1.
- **Statement completeness** holds only if every decision path emits — a tested
  invariant (mirrors argus §7).

## 9. Test plan (LiteSVM)

- Each privileged instruction enforces its specific role; a Cashier cannot create
  a campaign or clawback; grant/revoke is timelocked; non-admin role change
  rejected.
- Per-operator + per-merchant daily mint caps enforced; breach auto-freezes earn
  while redeem/clawback still succeed; day rollover resets.
- Config lifecycle: propose → approve (self-approval rejected) → timelock →
  activate re-tunes earn rate; rollback restores; pin blocks further change while
  freeze stays alive.
- `anchor_merchant_statement` by Reporter anchors root + count; re-anchor rejected;
  non-reporter rejected.
- Dual-control clawback above threshold requires both signers; recovery path.

## 10. Phased rollout

1. **`MerchantRoles` RBAC/SoD + per-operator daily issuance cap** — the keystone:
   least-privilege + issuance blast-radius cap in one account; closes the
   rogue-operator drain path. Near-verbatim argus port + the `can_operate`
   fallback shim.
2. **Governed config lifecycle** — removes the instant self-write foot-guns
   (clawback cap, earn rate).
3. **Merchant decision statements** — the examinable economic ledger.
4. **Dual-control clawback + recovery.**

## 11. Open questions

- Does `MerchantRoles` subsume `operators[4]` entirely at mainnet (migration) or
  coexist forever as the free-tier fallback? (Recommend: coexist; deprecate.)
- Fold `IssuanceControl` into `MerchantRoles` (fewer accounts) vs. separate
  (cleaner separation) — lean separate for auditability.
- Owner-recovery model: true re-key ceremony vs. governance operator-reset —
  resolve before touching `authority`.
- Statement anchoring cadence + indexer ownership (protocol vs. merchant vs.
  auditor) — the on-chain root is the trust primitive regardless (as argus §11).
