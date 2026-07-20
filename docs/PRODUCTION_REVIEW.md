# VESTA — Instruction-by-instruction review & production-readiness findings

Scope: every instruction file across `vesta_core`, `argus`, `aegis`, reviewed
for purpose (rationale) and defects/risks (findings). Then a consolidated list
of what is missing to ship to production.

Severity: 🔴 critical (correctness/security/blocks launch) · 🟠 high ·
🟡 medium · 🔵 low / nit.

---

## vesta_core

### `init_config.rs` — create the singleton global Config (admin + pause)
- 🟠 **Permissionless init / first-caller-wins.** `init_config` has no gate; on a
  fresh deploy whoever calls first becomes `admin`. The deployer must call it
  atomically post-deploy, or it should be gated to the program's upgrade
  authority (BPF loader `program_data.upgrade_authority`).
- 🔵 Event records only `admin`, no timestamp/slot context.

### `migrate_config.rs` — one-shot v1→v2 in-place Config realloc
- 🟡 **One-shot dead code in the prod binary.** After it runs once it is inert
  weight and a confusion vector; strip (or feature-gate) before mainnet.
- 🔵 Hand-rolled offset parsing is correct but brittle to layout assumptions.

### `admin.rs` — two-step admin transfer + global pause
- 🟠 **Single-key admin = centralization / key-compromise blast radius.** Admin
  can pause the whole protocol, take over via two-step, and flip merchant
  `verified`. Production needs a **multisig** (e.g. Squads) and ideally a
  **timelock** on sensitive actions.
- 🟡 No validation `new_admin != current` / `!= default` (low risk — default
  can't sign the accept).
- 🔵 `PausedSet` doesn't record which admin acted.

### `register_merchant.rs` — merchant + Token-2022 mint (4 exts) + metadata + treasury
- 🔴 **One merchant per wallet.** PDA `["merchant", authority]` hard-caps a wallet
  to a single merchant and blocks authority rotation. Enterprises run many
  brands/locations → need `["merchant", authority, merchant_id]` (or a global
  registry id). *(This is the "manage many records per wallet" requirement.)*
- 🟠 **No merchant delete / deregister.** CRUD "delete" is absent; no offboarding
  or rent reclaim for a merchant + its mint.
- 🟡 **Token metadata is immutable after registration.** No instruction updates
  the mint's `name`/`symbol`/`uri` (Token-2022 supports it) — a rebrand is
  impossible. `set_token_attribute` only adds `additional_metadata` keys.
- 🟡 **Decay rate immutable.** `InterestBearingConfig` rate can be updated
  on-chain, but there is no `update_decay_rate` instruction.
- 🟡 `update_merchant` only touches `base_earn_rate`; `name` on `Merchant` and in
  token metadata can drift and neither is editable.
- 🔵 Treasury is fixed to the authority's ATA (no alternate treasury).

### `merchant_admin.rs` — operators, pause, verified, profile, clawback cap
- 🟡 **Operator scope is inconsistent.** Operators can run `earn`/`clawback` (seeds
  use `merchant.authority`) but **cannot** create campaigns/offers/achievements
  or finalize the guard (those re-derive `["merchant", signer]`, owner-only).
- 🔵 `verify_merchant` emits no actor; category is an unvalidated opaque `u8`.

### `earn_points.rs` — streak earn (`earn_points`) + governed campaign earn
- 🟡 **Quest can "complete" without paying.** If the campaign budget / per-customer
  cap clamps the bonus to 0 at the moment `visits == quest_target`, the quest is
  marked `completed` and the reward is lost forever.
- 🟡 **Streak/`preview_total_bps` duplicates `accrue`'s streak math** — two copies
  can drift.
- 🟡 **No protocol issuance ceiling.** Beyond `MAX_EARN_PER_TX`, a merchant (or a
  compromised operator) can mint unbounded points across many txs. Bounded to
  its own mint, but there's no rate limit.
- 🔵 `visit_day` must equal today; merchant controls call timing (can pad its own
  streaks — self-affecting only).

### `campaigns.rs` — create/update/close campaigns (MULTIPLIER / FLAT_BONUS / QUEST)
- 🟡 **`points_budget` is notional, not escrowed.** It caps minting; no funds are
  locked. Fine, but must be documented so merchants don't treat it as a reserve.
- 🟡 No cross-field sanity (e.g. `per_customer_cap` < `flat_bonus` silently clamps
  every first earn).
- 🔵 `update_campaign` cannot early-terminate except via `paused`/close.

### `achievements.rs` — soulbound (NonTransferable) badge mint + double-grant guard
- 🟠 **Heavy single-tx build** (create mint → 2 exts → metadata → ATA → mint →
  revoke authority). Currently passes, but it is **CU/stack-sensitive** and was
  **not re-benchmarked after `Merchant`/`CustomerProfile` grew** — regression
  risk; needs an explicit CU/stack budget test.
- 🟠 **`Merchant.badges_issued` is never incremented** (dead stat — set to 0 at
  register, updated nowhere). Only `achievement.badge_count` moves.
- 🟡 No achievement **update or close** (can't retire/edit a badge definition).
- 🔵 Only `lifetime_earned` threshold; no tier/streak/spend criteria.

### `offers.rs` — catalog offer + redeem (burn) + receipt
- 🟠 **`Merchant.lifetime_redemptions` is never incremented** (dead stat — only the
  per-customer `profile.lifetime_redemptions` moves). Confirmed dead field.
- 🟠 **`redeem_offer` ignores `merchant.paused`** (only checks `config.paused`) —
  inconsistent with `earn_points`, so a paused merchant's offers still redeem.
- 🟡 **No offer time window** (offers live until closed; campaigns have windows).
- 🟡 **No per-customer redemption limit** — one wallet can drain `supply`.
- 🔵 Receipt index keyed on `profile.lifetime_redemptions`; fine, monotonic.

### `koinon.rs` — alliances, members, UI-denominated cross-swaps
- 🟠 **Swap mint bypasses `MAX_EARN_PER_TX`.** Leg B mints on the destination mint
  bounded only by the member's inbound daily budget, not the earn cap.
- 🟡 **`fee_bps` is a haircut, not revenue.** The spread reduces `ui_out` (less
  minted) but is collected nowhere — "alliance monetization" is a stub (no
  alliance treasury).
- 🟡 **`AllianceMember.active` is write-once `true`** — never toggled; effectively
  dead (leave closes the account instead).
- 🟡 **Rate bounds aren't retroactive** — tightening `min/max_rate_bps` doesn't
  re-validate existing members.
- 🔵 Swap needs a client `ComputeBudget` bump (2 `UiAmountToAmount` CPIs); silent
  failure if omitted.

### `clawback.rs` — enterprise clawback via PermanentDelegate (audited transfer)
- 🟡 **Ignores `merchant.paused`** (intentional — remediation must work while
  paused) but this is implicit; document it.
- 🔵 `customer_profile` is `init_if_needed` at merchant expense for a
  swap-only holder with no profile.
- 🔵 No per-customer clawback cap (only per-merchant daily) — by design.

### `finalize_transfer_guard.rs` — burn the mint's transfer-hook authority to None
- 🟠 **Irreversible hook lock.** Once finalized, the mint's transfer-hook program
  can never change. If `argus` has a latent bug, that mint is permanently stuck
  with it — there is no per-mint hook upgrade path. (Intentional immutability,
  but a real operational risk to weigh before finalizing on mainnet.)
- 🔵 Owner-only (operators can't finalize) — appropriate.

### `set_token_attribute.rs` — additional_metadata key/value on the point token
- 🟡 **No attribute removal** (set/overwrite only) — CRUD delete missing.
- 🟡 **Unbounded attribute count** → the mint account grows without limit; rent
  paid per add, no cap.
- 🔵 Owner-only (consistent with admin ops).

---

## argus

### `initialize_transfer_guard.rs` — create GuardConfig + ExtraAccountMetaList
- 🟠 **aegis issuer + program are frozen at init** (baked into the EAML). If the
  aegis issuer key rotates or aegis is redeployed to a new id, attestation
  gating breaks and cannot be repaired (especially after `finalize`). Tight
  cross-program coupling.
- 🟡 **7 fixed extra accounts on every transfer** (incl. attestation/list slots
  even when unused) — CU + tx-size overhead for all hooked transfers.

### `open_wallet_state.rs` — per-(mint,owner) velocity state, customer-signed
- 🟡 **Pre-open friction.** First-time senders need a separate tx to create state;
  the SDK must bundle it or gifts fail closed.
- 🔵 Non-closable by design (anti-reset).

### `execute.rs` — the transfer-hook decision pipeline
- 🟠 **Program-owned-destination filter is best-effort** — system-owned PDAs
  (many AMM vault authorities) pass; not a true DEX/pool block.
- 🟠 **Whole-mint fragility.** Every transfer requires all resolved extras; if a
  client or a future Token-2022 change omits one, *all* transfers of that mint
  fail. Fail-closed is correct but brittle.
- 🟡 **Per-transfer CU cost** (reads GuardConfig, wallet state, and a foreign
  aegis account on the attestation path) — not benchmarked against mainnet CU.
- 🔵 Wallet-splitting bypasses per-wallet caps (disclosed; `max_wallet_balance`
  only blunts the receiving side).

### `admin.rs` (argus) — configure_policy / pause / two-step authority / list entries
- 🟡 **Guard authority decoupled from vesta_core merchant ownership.** It's the
  merchant *wallet* at init; if merchant control changes in vesta_core, the argus
  guard authority does not follow.
- 🟡 **Unbounded allow/deny list entries** (authority-funded rent).

### `policy.rs` (argus) — InitialPolicy / PolicyUpdate + validation
- 🔵 Validation is sound (`per_tx ≤ daily`, attestation issuer required when the
  flag is set, unknown flag bits rejected). No findings of substance.

---

## aegis

### `issuer.rs` — Issuer create + operator + pause + two-step authority
- 🟡 **One issuer per creator wallet** (`["issuer", authority]`) — same
  multi-record limit as merchants; a wallet can run only one issuer.
- 🔵 `accept_issuer_authority` clears the operator (documented design choice).

### `attestation.rs` — issue / update / revoke / close attestations
- 🟡 **Revocation is reversible.** `update_attestation` clears `revoked` — a
  revoked credential can be silently reinstated (audit concern).
- 🟡 **No batch issuance.** A geofenced drop to N wallets is N transactions —
  poor scalability for large campaigns.
- 🔵 `schema`/`value` are free-form; no on-chain schema registry (trust is the
  merchant's mask config — disclosed).

---

## Cross-cutting: what's missing to launch to production

### Security & governance (blockers)
1. 🔴 **No security audit.** Mandatory before mainnet value.
2. 🔴 **Single-key admin & single-key program upgrade authority.** Move both to a
   multisig (Squads) + timelock; document an upgrade/rollback policy.
3. 🔴 **Multi-record ownership + full CRUD.** One merchant/issuer per wallet;
   no delete for merchant/achievement/attribute; add id-based PDAs and complete
   create/read/update/**delete** + lifecycle (activate/deactivate/transfer).
4. 🟠 **`init_config` front-run** — gate to the upgrade authority or init atomically.
5. 🟠 **`finalize_transfer_guard` irreversibility** — decide/​document the mainnet
   policy; a hook bug becomes permanent per mint.

### Correctness / data integrity
6. 🟠 **Dead stat fields**: `Merchant.lifetime_redemptions` and
   `Merchant.badges_issued` are never updated — wire them or remove (dashboards
   will read zeros).
7. 🟠 **`redeem_offer` doesn't honor `merchant.paused`** — inconsistent gating.
8. 🟡 **Quest-completes-without-payout** edge when budget/cap clamps to 0.
9. 🟡 **`AllianceMember.active`** is a dead flag.
10. 🟡 **Swap bypasses the earn cap**; **alliance `fee_bps`** collects nothing.

### Engineering rigor
11. 🟠 **CU & stack budget audit for every instruction.** `earn_points_campaign`
    already overflowed the 4 KB stack once (fixed by boxing); `grant_achievement`
    and `swap_points` are the next suspects — add CU/stack assertions.
12. 🟠 **No real-validator / wallet integration tests.** Only LiteSVM unit tests;
    no `anchor test` against a validator, no click-tested Phantom flows, Python
    SDK flows are stubbed.
13. 🟡 **Coverage gaps** (untested paths): `init_config` front-run,
    `migrate_config`, vesta admin two-step, `update_merchant`, `verify_merchant`,
    `set_token_attribute` auth, alliance `fee_bps` math, alliance stats,
    `set_clawback_cap` day-rollover.
14. 🟡 **Metadata mutability** — no update path for token name/symbol/uri or decay.
15. 🟡 **Rent/economics doc** — who pays for profiles/wallet-state/receipts, which
    accounts are (non-)closable, and reclamation paths.
16. 🟡 **Reproducible/verified builds** (`solana-verify`) for the mainnet badge.

### Ops & product
17. 🟠 **Deployed devnet programs are STALE** — the enriched `vesta_core` and
    `argus v2` are built + tested but not redeployed (blocked on devnet SOL).
18. 🟡 **Threat model + incident-response runbook** (pause procedures exist; the
    playbook doesn't).
19. 🟡 **Indexer / event pipeline** — confirm every state mutation emits an event
    and stand up indexing for dashboards.
20. 🟡 **UI + Python SDK** must be extended to the new instruction surface
    (campaigns kinds, operators, alliance governance, clawback controls).
21. 🟡 **aegis↔argus coupling** — a documented key-rotation / redeploy story for
    the trusted issuer.

### Positives (already solid)
- Fail-closed transfer hook; delegation-proof velocity state; two-step authority
  everywhere; defensive PDA-prefund handling; checked arithmetic enforced by
  workspace lints; soulbound badges with a burn-proof double-grant guard;
  reason-coded audit events; 44 LiteSVM tests green.
