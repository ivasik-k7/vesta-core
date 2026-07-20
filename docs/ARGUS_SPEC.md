# ARGUS — Technical Specification

> The hundred-eyed guard: a programmable transfer-policy engine at the Solana
> token layer. This document specifies argus as a standalone, enterprise-grade
> product — the rules that travel with a VESTA point token wherever it goes.
>
> Companion to `TECHNICAL_SPEC.md` (the vesta_core protocol). Where the two
> overlap, the argus-specific detail here governs.

- Status: **v2 design** — extends the deployed v1 (three instructions) into a
  configurable policy engine. Sections tagged `[v1]` describe shipped behavior;
  `[v2]` is the target; `[VERIFY]` marks claims for the fact-check pass.
- Deployed (devnet): `argus` `9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx`
- Interface: `spl-transfer-hook-interface` 2.1.0 · `spl-tlv-account-resolution`
  0.11.1 · Token-2022 (`spl-token-2022-interface` 2.1.0) · Anchor 1.1.2

---

## 0. Why argus is its own program

Token-2022's transfer-hook extension names **one program** per mint that
Token-2022 CPIs on every transfer. That program is the only place where
"who may move this token, to whom, how much, how often" can be enforced
uniformly — wallets, DEXes, other dApps all trigger it. vesta_core governs
mint/burn (earn/redeem/swap); argus governs **free-floating transfers**. The
split is not incidental — it is the entire security model: value cannot escape
the intended loyalty flows without passing argus first.

v1 hard-codes one policy (a daily gift cap + a few allow rules). The enterprise
reality is that different merchants need different policies: a coffee shop wants
generous gifting; an airline wants strict anti-resale controls; a regulated
issuer needs jurisdiction gating. **v2 turns argus from a fixed rule set into a
per-mint, merchant-configurable policy engine** — without ever giving up the
fail-closed guarantee.

---

## 1. Mandate and design principles

argus decides, for each transfer, **allow or reject**, and emits a reasoned
audit record. It never moves funds (it cannot — see §4).

1. **Fail-closed.** Any ambiguity — missing state, unparseable config,
   unexpected account shape — rejects the transfer. A guard that fails open is
   not a guard.
2. **De-escalation-aware.** The hook runs with every account read-only and
   non-signer (§4). Every design choice respects that: no account created,
   no fund moved, no signature assumed inside `execute`.
3. **Config-driven, not code-driven.** Policy lives in on-chain accounts the
   merchant tunes, not in program constants. Changing a cap is a transaction,
   not a redeploy.
4. **Observable by default.** Every decision emits a structured event with a
   reason code, so issuers and indexers get a complete, queryable audit trail.
5. **Composable outward.** Guard config and wallet-policy state are public,
   documented accounts any dApp can read to predict whether a transfer will
   clear — no simulation required.
6. **Least authority.** The per-mint guard authority can tune policy but can
   never mint, burn, freeze, or move a customer's tokens. Its blast radius is
   bounded to its own mint's transfer rules.

---

## 2. Account model

All PDAs under the argus program id unless noted. Canonical bumps stored.

### 2.1 `GuardConfig` `[v2]` — the per-mint policy

Seeds `["guard", mint]`. Created at `initialize_transfer_guard`; tuned by the
guard authority thereafter.

| Field | Type | Meaning |
|---|---|---|
| `mint` | `Pubkey` | the hooked mint this config governs |
| `authority` | `Pubkey` | who may update policy (the merchant PDA from vesta_core) |
| `pending_authority` | `Option<Pubkey>` | two-step authority rotation |
| `treasury` | `Pubkey` | merchant treasury ATA — always an allowed destination |
| `paused` | `bool` | per-mint transfer freeze (peer transfers only; §6) |
| `daily_gift_cap` | `u64` | raw units a wallet may send per UTC day (0 = gifting off) |
| `per_tx_cap` | `u64` | max raw units in a single peer transfer (0 = no per-tx limit) |
| `cooldown_secs` | `u32` | minimum seconds between a wallet's peer transfers |
| `max_wallet_balance` | `u64` | reject transfers that would push destination over this (0 = off) |
| `flags` | `u16` | bitset: BLOCK_PROGRAM_OWNED, ALLOWLIST_ONLY, REQUIRE_ATTESTATION, … |
| `list_root` | `Option<Pubkey>` | optional allow/deny list authority set (§2.4) |
| `attestation_program` | `Option<Pubkey>` | aegis (or compatible) program for compliance gating (§7) |
| `bump` | `u8` | |

Rationale: one account, read once in `execute`, holds the whole policy. Cheap
to fetch; cheap to reason about off-chain.

### 2.2 `WalletPolicyState` `[v2]` — per-(mint, owner) counters

Seeds `["wstate", mint, owner]`. Supersedes v1's `GiftLedger`. Pre-created via
`open_wallet_state` (in-hook creation is impossible — §4).

| Field | Type | Meaning |
|---|---|---|
| `day` | `u32` | current UTC day of the counters below |
| `sent_today` | `u64` | raw units sent as peer transfers today |
| `last_transfer_at` | `i64` | unix ts of the wallet's last peer transfer (cooldown) |
| `transfers_today` | `u16` | count, for per-day transfer-count limits |
| `bump` | `u8` | |

Deliberately **non-closable** — closing + reopening would reset the daily
counters; the locked rent is the anti-reset bond (unchanged from v1's ledger).

### 2.3 `ExtraAccountMetaList` `[v1]`

Seeds `["extra-account-metas", mint]`, interface-defined. Resolves the extra
accounts Token-2022 passes into `execute`: the `WalletPolicyState` (writable,
seeded from the source-owner data), the `GuardConfig` (read), the destination
owner wallet (pubkey-data deref), the treasury (literal), and — when
`REQUIRE_ATTESTATION` is set — the attestation account (§7). `[VERIFY]` that a
conditionally-required meta can be modeled (fixed meta list; the attestation
slot is always present but ignored unless the flag is set).

### 2.4 `PolicyList` + `PolicyListEntry` `[v2]` — allow/deny sets

Optional. `PolicyList` seeds `["list", mint]` holds a mode (ALLOW | DENY) and a
count; `PolicyListEntry` seeds `["entry", list, target]` marks one address.
`execute` checks membership via the deterministically-derived entry PDA passed
as an extra meta (existence = membership). Keeps `execute` O(1) — no scanning.
`[VERIFY]` that an entry PDA can be supplied as a pubkey-data-derived meta over
the destination-owner key.

---

## 3. Instruction reference

### 3.1 `initialize_transfer_guard(policy: InitialPolicy)` `[v1→v2]`

Creates the `ExtraAccountMetaList` **and** the `GuardConfig`. Strictly
authorized (v1 chain retained): the merchant authority signs; the `Merchant`
account is verified `owner == vesta_core`, re-derived from
`["merchant", authority]`, and `merchant.point_mint == mint`. v2 additionally
writes the initial policy into `GuardConfig`. Idempotent guard: fails if the
EAML already exists (init-once; front-run protection). Emits
`TransferGuardInitialized`.

### 3.2 `configure_policy(update: PolicyUpdate)` `[v2]`

Guard-authority-only. Partial update (each field `Option`) of `GuardConfig` —
caps, cooldown, flags, list root, attestation program. Every change emits
`PolicyConfigured` with the diff for auditability. Range-validated (e.g.
`per_tx_cap <= daily_gift_cap` when both set).

### 3.3 `set_guard_paused(paused: bool)` `[v2]`

Guard-authority-only per-mint circuit breaker. Distinct from vesta_core's
protocol pause: this freezes **peer transfers of one mint** while leaving
clawback/treasury flows (rules 1–2) open, so an issuer can stop secondary
movement of a compromised campaign without bricking refunds.

### 3.4 `transfer_guard_authority` / `accept_guard_authority` `[v2]`

Two-step rotation of the guard authority (typo-proof), mirroring the protocol
admin pattern.

### 3.5 `manage_list(op: Add | Remove, target: Pubkey)` `[v2]`

Guard-authority-only. Creates/closes a `PolicyListEntry`. Rent for an added
entry paid by the authority; reclaimed on remove.

### 3.6 `open_wallet_state(mint)` `[v1→v2]`

Customer-signed, one-time. Creates `WalletPolicyState` for `(mint, signer)`.
In-hook creation is impossible (§4), so first-time senders bundle this with
their transfer; the SDK does it automatically. (v1 name: `open_gift_ledger`.)

### 3.7 `execute(amount)` `[v1→v2]`

The interface entry point Token-2022 CPIs. Discriminator
`ExecuteInstruction::SPL_DISCRIMINATOR_SLICE`. The decision pipeline is §5.

### 3.8 `finalize_guard()` — lives in vesta_core, documented here

vesta_core burns the mint's transfer-hook **authority** to `None` after the
guard is initialized, so the hook program can never be repointed. Note the
distinction: the *hook-program-id authority* (frozen) vs. the *GuardConfig
authority* (retained, so policy stays tunable). Freezing one does not freeze the
other — a deliberate, documented separation.

---

## 4. Execute-time facts the engine relies on (verified)

Carried over and re-affirmed from the vesta_core spec's round-2 resolution:

1. The hook fires on `transfer_checked` / `transfer_checked_with_fee`,
   including permanent-delegate transfers (delegate arrives as authority), and
   **never** on mint/burn.
2. Accounts arrive privilege-de-escalated: source, mint, destination, authority
   are read-only non-signers. The hook cannot move funds, sign, or (usefully)
   create accounts inside `execute` — hence pre-created `WalletPolicyState`.
3. Hook-owned extra accounts **can be writable** (`ExtraAccountMeta`
   `is_writable = true`) — the `WalletPolicyState` update path.
4. Extra metas derive from transfer-account **data**:
   `Seed::AccountData { account_index: 0 (source), data_index: 32, length: 32 }`
   for the source-owner-keyed state (delegation-proof); `PubkeyData::AccountData`
   over the destination (offset 32) to reach the destination-owner wallet.
5. Token-2022 rejects legacy unchecked `transfer` for hook mints — only the
   `*_checked` variants reach `execute`, so amount + decimals are always present.
6. Missing meta accounts do not fail inside `invoke_execute`; argus therefore
   hard-requires its extras in the `Execute` context, so omission fails closed.

---

## 5. The decision pipeline

`execute(amount)` evaluates in strict order; the first terminal rule wins.
Every branch emits an event carrying a `reason_code` (§10).

```
0. Load GuardConfig (account #, read). Unparseable → REJECT (config_error).
1. authority == mint.permanent_delegate?           → ALLOW  (issuer_flow)
2. destination == config.treasury?                 → ALLOW  (treasury_flow)
3. config.paused?                                  → REJECT (mint_paused)
4. amount == 0?                                    → ALLOW  (noop) [VERIFY policy]
5. flags.BLOCK_PROGRAM_OWNED and dest-owner is
   program-owned (owner != system_program)?        → REJECT (program_owned_dest)
6. flags.ALLOWLIST_ONLY and dest not in allow list? → REJECT (not_allowlisted)
   (DENY list mode: dest in deny list → REJECT deny_listed)
7. flags.REQUIRE_ATTESTATION and attestation
   missing/expired/wrong-subject?                   → REJECT (attestation_failed)
8. per_tx_cap set and amount > per_tx_cap?          → REJECT (per_tx_exceeded)
9. max_wallet_balance set and dest_balance + amount
   > max_wallet_balance?                            → REJECT (balance_cap)
10. cooldown_secs set and now - state.last < cd?    → REJECT (cooldown)
11. roll WalletPolicyState by day; sent_today +
    amount > daily_gift_cap?                        → REJECT (daily_cap)
12. else: update state (sent_today, last, count),
    ALLOW                                            (gift)
```

Rules 1–2 short-circuit before touching `WalletPolicyState`, so issuer and
payment flows never require an opened state account (the v1 gift-then-clawback
correctness property, generalized).

**Load-bearing vs. best-effort.** Rules 11 (daily cap) and 8/9/10 (velocity)
are the hard guarantees. Rule 5 (program-owned) is best-effort — many AMM vault
authorities are system-owned PDAs and pass it — documented honestly, exactly as
v1 does; the velocity caps are what actually bound leakage.

---

## 6. Limits and velocity model `[v2]`

A single wallet's transfer behavior is shaped by five independent, composable
knobs, all per-mint:

- **daily_gift_cap** — total raw units out per UTC day. The v1 guarantee.
- **per_tx_cap** — ceiling on a single transfer (anti-whale, anti-drain).
- **transfers_today** cap — optional count limit (anti-spam / anti-dusting).
- **cooldown_secs** — minimum spacing between transfers (anti-burst).
- **max_wallet_balance** — reject transfers that would over-concentrate points
  in one destination (anti-hoarding / anti-sybil-aggregation).

Honest limitation (unchanged, now documented as policy): all counters are per
`(mint, source-owner)`. Splitting across N wallets multiplies limits by N. argus
raises the cost of leakage; it is not an identity system. `max_wallet_balance`
partially counters aggregation on the **receiving** side, which is new in v2 and
genuinely useful against a single hoarding address.

---

## 7. Compliance & attestation gating `[v2]` — the aegis hook

When `flags.REQUIRE_ATTESTATION` is set, `execute` requires an attestation
account (from `config.attestation_program`) proving the destination owner holds
a valid credential — e.g., a jurisdiction/region tag, an age band, or a KYC
tier. argus checks: owned by the configured program, subject == destination
owner, not expired, satisfies the required predicate encoded in `flags`.

This is how VESTA reaches the challenge's "spatial/geofenced token drops":
a geofenced campaign sets `REQUIRE_ATTESTATION` with a region predicate; only
wallets whose aegis attestation encodes the allowed region can receive the
drop. argus stays generic — it validates an attestation shape; **aegis** (§13)
issues them. Neither program knows the other's business logic; they compose
through a documented account layout.

`[VERIFY]` the attestation account can be supplied as a
destination-owner-derived meta, and that reading a foreign program's account in
`execute` is within CU budget.

---

## 8. Security model

Threats specific to a policy engine, and mitigations:

- **Guard front-run at init** → init-once EAML + authority chain (§3.1).
- **Policy tampering** → `configure_policy` gated to the guard authority; the
  authority is the merchant PDA, itself program-controlled.
- **Hook repointing** → vesta_core burns the hook-program authority post-init
  (§3.8); config authority stays separate and cannot repoint.
- **Counter reset via close/reopen** → `WalletPolicyState` non-closable.
- **Delegation laundering** → state keyed on source-owner *data*, not the
  authority account (a delegate cannot mint a fresh counter).
- **Cap bypass via wallet-splitting** → disclosed; `max_wallet_balance` blunts
  the receiving side; true fix needs identity (out of scope).
- **Config-account substitution** → `GuardConfig` supplied as a seeded meta
  (`["guard", mint]`); argus re-derives and rejects a mismatch.
- **List substitution** → entry PDAs are derived from `(list, target)`; a
  forged entry can't collide.
- **Attestation forgery** → argus checks owner-program + subject + expiry;
  trust is delegated to the configured attestation program, disclosed.
- **CU exhaustion** → the pipeline is O(1): fixed account reads, no loops. The
  attestation branch is the heaviest; measured and bounded in tests.
- **Reentrancy** → no token CPIs on the transferring accounts inside `execute`.
- **Pause griefing** → guard pause is authority-only and never blocks
  issuer/treasury flows, so it cannot brick refunds.

---

## 9. Interface compliance

- `execute` uses the interface discriminator; `#[instruction(discriminator =
  ExecuteInstruction::SPL_DISCRIMINATOR_SLICE)]` with `use
  spl_discriminator::SplDiscriminate;` in scope (the pattern shipped in v1).
- The EAML is initialized with `ExtraAccountMetaList::init::<ExecuteInstruction>`.
- Client transfers append the resolved extras; off-chain callers use
  spl-token's `createTransferCheckedWithTransferHookInstruction` (min version
  pinned in the SDK for pubkey-data meta resolution). On-chain CPIs that
  transfer (vesta_core clawback) pass the extras explicitly.

---

## 10. Observability

Every `execute` decision emits one event (log-based `emit!`, `emit_cpi!`
upgrade path documented):

`TransferAllowed { mint, source_owner, destination, amount, reason }` and
`TransferRejected { mint, source_owner, destination, amount, reason }` where
`reason` is a stable `u16` enum (issuer_flow, treasury_flow, gift, mint_paused,
program_owned_dest, not_allowlisted, deny_listed, attestation_failed,
per_tx_exceeded, balance_cap, cooldown, daily_cap, config_error).

Plus lifecycle events: `TransferGuardInitialized`, `PolicyConfigured` (diff),
`GuardPausedSet`, `GuardAuthorityProposed/Changed`, `ListEntryAdded/Removed`,
`WalletStateOpened`. This is the compliance-grade audit trail an enterprise
issuer needs: every allow/deny, with a reason, queryable by mint or wallet.

---

## 11. Composability

Guard state is public and documented so third parties integrate without
reading program source:

- **Pre-flight**: any dApp can fetch `GuardConfig` + a wallet's
  `WalletPolicyState` and compute whether a transfer of `amount` will clear —
  no simulation. This is a real integration surface (a wallet can grey out a
  gift that would exceed the cap).
- **Policy discovery**: `getProgramAccounts` over `GuardConfig` (memcmp on
  authority/flags) enumerates every mint's policy — an ecosystem-wide view of
  which loyalty tokens allow gifting, which are geo-gated, etc.
- **Reason codes** are a stable ABI for compliance tooling.

---

## 12. Testing strategy

LiteSVM (bundled Token-2022), per the vesta_core harness. Coverage:

- Each pipeline rule: a transfer that trips exactly that rule, plus the
  boundary (cap−1 allow, cap exact allow, cap+1 reject; cooldown edge; balance
  cap edge).
- Ordering: issuer/treasury short-circuit before state; paused blocks peer but
  not clawback; allowlist-only vs. deny-list modes.
- Config lifecycle: configure updates take effect on the next transfer;
  two-step authority; range validation rejects `per_tx > daily`.
- Fail-closed: omitted extras abort; unparseable config rejects; missing wallet
  state on a peer transfer rejects (not silently allows).
- Delegation-proof: delegated transfer spends the source-owner state.
- Attestation: valid passes, expired/wrong-subject/missing rejects.
- CU/size assertions on the heaviest path (attestation + list + state write).
- Devnet e2e: reproduce each reason code with a real transaction; links to
  README.

---

## 13. Synergy programs (the constellation)

argus is one eye. The enterprise value compounds when specialized programs
compose around it, each doing one thing and exposing a documented account
surface. Proposed, Greek-named to match the house style:

- **aegis** — *the shield.* An attestation issuer: signs region / age / KYC-tier
  credentials into per-subject accounts that argus (§7) and vesta_core campaigns
  gate on. Unlocks geofenced/spatial drops and regulated issuance. **Highest
  synergy** — turns argus from a velocity limiter into a compliance engine.
- **moira** — *the Fates.* An algorithmic rewards crank: permissionless keepers
  execute rule-based issuance (streak bonuses, decay-offset top-ups, quest
  completions) by CPI into vesta_core `earn`. Turns "automated algorithmic
  rewards" from a phrase into a program.
- **horae** — *the Hours/Seasons.* Time- and geo-windowed drop scheduler:
  defines claimable windows; issues the attestations horae+aegis jointly gate.
- **nomos** — *law.* An on-chain registry/index of merchants, alliances, and
  guard policies for discovery — the composability backbone a marketplace of
  loyalty programs needs.

Delivery order by value/effort: **aegis** first (directly amplifies argus and
hits an unaddressed challenge criterion), then **moira** (innovation showcase),
then nomos/horae. Each is a separate spec; this section is the map, not the
territory.

---

## 14. Migration from v1

The deployed v1 argus has three instructions and a hard-coded cap. v2 is
additive at the interface level (same `execute` discriminator, same EAML seeds)
but changes account layout (`GuardConfig` replaces the implicit constant;
`WalletPolicyState` replaces `GiftLedger`). Devnet state is disposable
(challenge policy): redeploy under the same program id, re-run
`initialize_transfer_guard` with an initial policy, and reseed the demo. No
in-place migration of v1 ledgers — they are devnet fixtures. Documented as
accepted debt, exactly as vesta_core's Config migration was handled.

---

## 15. Phased delivery

| Phase | Scope |
|---|---|
| A1 | `GuardConfig` + `configure_policy` + `set_guard_paused`; migrate `execute` to read config (daily cap now tunable). Two-step guard authority. |
| A2 | Full velocity model: per_tx_cap, cooldown, transfers_today, max_wallet_balance. `WalletPolicyState` replaces `GiftLedger`. |
| A3 | Allow/deny lists (`PolicyList`/`Entry`, `manage_list`). |
| A4 | Attestation gating (`REQUIRE_ATTESTATION`) against the aegis account shape. |
| A5 | Observability pass: full reason-coded event set; pre-flight read helpers in the SDK; policy-discovery queries. |

A1–A2 are the enterprise core (configurable policy + velocity). A3–A5 layer
compliance and composability. aegis (§13) is specced separately and gates A4.

---

## 16. Open questions / tradeoffs

- **Fixed vs. dynamic meta list.** A fixed EAML (attestation/list slots always
  present, ignored unless flagged) is simpler and predictable but wastes a few
  account slots per transfer; a dynamic list per policy is leaner but harder to
  keep fail-closed. Leaning fixed. `[VERIFY]` the account-count/tx-size budget.
- **Config in one account vs. split.** One `GuardConfig` is cheap to read but
  every `execute` deserializes the whole thing; at ~20 fields this is trivial CU.
  Keep unified.
- **Attestation trust.** argus trusts whatever program `config.attestation_program`
  names. That is a per-mint decision by the merchant; argus discloses it, does
  not adjudicate it. A malicious merchant naming a permissive attestor only
  harms their own mint's guarantees.
- **max_wallet_balance and legitimate whales.** A power user legitimately
  accumulating points hits the cap. It is opt-in per mint; off by default.
- **Rolling window vs. UTC-day reset.** UTC-day is simple and matches vesta_core
  streaks; a true rolling 24h window needs a ring buffer. Keep UTC-day; document.
