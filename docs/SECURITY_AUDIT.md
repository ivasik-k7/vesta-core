# VESTA — Security Audit

**Target:** `vesta_core`, `argus`, `aegis` (Anchor 1.1.2 / Token-2022)
**Commit reviewed:** v2 working tree (post-README rework)
**Method:** adversarial source review — fan-out across five independent reviewers by program and vulnerability class, every reported finding re-verified line-by-line against source before inclusion.
**Result:** 1 High, 3 Medium, 5 Low, 3 Informational (new). No Critical. The cross-mint swap is provably value-conserving and the Token-2022 extension authorities contain no backdoor.

> [!IMPORTANT]
> **This is an internal, AI-assisted adversarial review — not an independent third-party firm audit.**
> It does **not** satisfy the "external security audit" gate on the [mainnet roadmap](../README.md#roadmap).
> Treat it as a rigorous pre-audit pass that hardens the code and narrows what a paid firm must examine.
> Do not deploy with real value on the strength of this document alone.

---

## Scope

| Program | Files reviewed |
|---|---|
| `vesta_core` | init_config, migrate_config, admin, register_merchant, merchant_admin, earn_points, campaigns, offers, achievements, clawback, koinon, set_token_attribute, finalize_transfer_guard, state, events, lib |
| `argus` | execute, initialize_transfer_guard, admin, policy, open_wallet_state, constants, state, lib |
| `aegis` | issuer, attestation, state, lib, constants |

Vulnerability classes examined: missing signer/owner checks, account substitution (non-canonical PDAs), PDA seed collision, authority confusion (owner vs operator), fail-open paths in the transfer hook, cross-program trust (argus↔aegis), value conservation in swaps, Token-2022 extension-authority correctness, `init_if_needed` reinitialization, arithmetic/precision/rounding, account close-and-revive, day-rollover and clock handling, and audit-trail integrity.

## Findings summary

| ID | Severity | Title | Status |
|---|---|---|---|
| **H-1** | 🔴 High | `argus::execute` is not bound to a real transfer — velocity-state DoS + audit forgery | Open |
| **M-1** | 🟠 Medium | Operator clawback is unbounded by default (kill-switch ships disarmed) | Open |
| **M-2** | 🟠 Medium | Merchant owner authority is non-rotatable (no recovery / transfer path) | Open |
| **M-3** | 🟠 Medium | Stale `CampaignProgress` bleeds into a recreated campaign id | Open |
| **L-1** | 🟡 Low | Cooldown fails open under a backward clock (`unwrap_or(i64::MAX)`) | Open |
| **L-2** | 🟡 Low | Global pause does not cover decay-rate / token-metadata mutation | Open |
| **L-3** | 🟡 Low | Member inbound swap budget is self-set and unbounded (no alliance co-sign) | Open |
| **L-4** | 🟡 Low | `merchant.customer_count` under-counts redeem-first / clawback-first customers | Open |
| **L-5** | 🟡 Low | Quest can become structurally impossible to complete when cap < reward | Open |
| **I-1** | 🔵 Info | Stale `update_attestation` doc comment contradicts the (safe) code | Open |
| **I-2** | 🔵 Info | `verify_merchant` target account is unconstrained (safe, admin-gated) | Open |
| **I-3** | 🔵 Info | Sub-cent UI-amount rounding wobble in swaps (not profitably extractable) | Noted |

---

## Detailed findings

### H-1 — 🔴 `argus::execute` is not bound to a real transfer context

**Location:** [`argus/src/instructions/execute.rs`](../programs/argus/src/instructions/execute.rs) — `Execute` struct (L20–49) and commit block (L214–219).

The transfer hook never verifies it is running inside a genuine Token-2022 transfer: it does not check the source account's `TransferHookAccount.transferring` flag, does not introspect the instructions sysvar, and requires **no signer** on any account. `source_owner` is read blindly from bytes 32–64 of the `source` account ([L335–342](../programs/argus/src/instructions/execute.rs#L335)), and `wallet_state` is a writable `UncheckedAccount`.

**Exploit.** An attacker crafts a top-level instruction directly to `execute` with `amount = daily_gift_cap`, passing a victim `V`'s public ATA as `source` (its bytes 32–64 already hold `V`), the real `mint`/`guard_config`/`extra_account_meta_list` PDAs, `V`'s real writable `wallet_state` PDA (writable needs no signature), and an attacker-owned destination of the same mint. With no allowlist/attestation configured (or a compliant attacker destination) the flow reaches the commit block and writes into **V's** state: `sent_today = daily_gift_cap`, `transfers_today += 1`, `last_transfer_at = now`. Repeat per day / per cooldown window.

**Impact.** *Availability and integrity, not theft.*
- **DoS:** any wallet's daily volume budget / transfer-count can be exhausted and its cooldown continuously refreshed, blocking that wallet's legitimate peer transfers indefinitely, at the cost of only fees. The velocity counters — the guard's one piece of mutable cross-user state — become attacker-writable.
- **Audit forgery:** `decide()` ([L228](../programs/argus/src/instructions/execute.rs#L228)) emits `TransferDecision{allowed, reason}` events for transfers that never happened (set `authority` = the mint's public permanent delegate to forge `ISSUER_FLOW` "allowed" records). Spec §10 treats these as the authoritative compliance trail.

A real transfer runs its own `execute`, so this does **not** let a genuinely-blocked transfer succeed or move funds — but it defeats the two controls the guard's value proposition rests on. *(Independently reported by two reviewers.)*

**Fix.** Refuse invocation outside a real transfer. Unpack `source` as `StateWithExtensions::<Account>`, require `source.owner == spl_token_2022_interface::ID`, require the source mint equals `mint`, and require `get_extension::<TransferHookAccount>()?.transferring` to be set (Token-2022 sets this flag only for the duration of a genuine transfer). Reject otherwise (fail closed). This leaves the real hook path untouched.

### M-1 — 🟠 Operator clawback is unbounded by default

**Location:** [`clawback.rs:94–99`](../programs/vesta-core/src/instructions/clawback.rs#L94), [`state.rs:59–73`](../programs/vesta-core/src/state.rs#L59), [`register_merchant.rs:327`](../programs/vesta-core/src/instructions/register_merchant.rs#L327).

`clawback` authorizes via `merchant.can_operate(signer)` — owner **or** any of the ≤4 operators (the keys the design keeps online at the POS). The only limiter is `clawback_daily_cap_raw`, whose comment states it "bounds the blast radius of a compromised operator key" — but `register_merchant` initializes it to `0`, and `0` means *unlimited* ([clawback.rs:116](../programs/vesta-core/src/instructions/clawback.rs#L116)). Setting a cap is opt-in via the owner-only `set_clawback_cap`.

**Impact.** Out of the box, a single compromised operator hot key can confiscate the entire customer base's balances into the treasury with no daily limit until the owner intervenes. The destination is pinned to `merchant.treasury` by `has_one`, so this is confiscation/griefing of that merchant's own customers — not theft to an attacker wallet — but the mitigation the design relies on ships disarmed.

**Fix.** Seed a sane non-zero `clawback_daily_cap_raw` at registration, and/or grant clawback as a separate operator capability from earn/campaign rights.

### M-2 — 🟠 Merchant owner authority is non-rotatable

**Location:** [`state.rs` `Merchant`](../programs/vesta-core/src/state.rs) (no `pending_authority`); no `transfer_merchant_authority` instruction in `lib.rs`.

The merchant owner is baked into the PDA seeds `["merchant", authority, id]` and is simultaneously the mint authority, permanent delegate, metadata update authority, and interest-rate authority. There is no two-step transfer or recovery path — asymmetric with `Config.admin`, `Alliance.authority`, `Issuer.authority`, and the argus `GuardConfig.authority`, all of which rotate.

**Impact.** If the owner key is lost or compromised there is no recovery and operators cannot even be revoked (that too is owner-only). A clean "sell / transfer the merchant" operation is impossible, and the argus guard authority can drift away from a merchant owner that can never itself move.

**Fix.** Add `pending_authority` to `Merchant` and a two-step `transfer_merchant_authority` / `accept_merchant_authority` (mirroring the alliance/issuer pattern). Note this interacts with the PDA seed design — likely store the owner in the account and derive the PDA from a stable id, or document the tradeoff explicitly.

### M-3 — 🟠 Stale `CampaignProgress` bleeds into a recreated campaign id

**Location:** [`campaigns.rs:204` (`close = authority`)](../programs/vesta-core/src/instructions/campaigns.rs#L204), [`campaigns.rs:74` (`init`, id-based seeds)](../programs/vesta-core/src/instructions/campaigns.rs#L74), [`earn_points.rs:284` (`fresh_progress`)](../programs/vesta-core/src/instructions/earn_points.rs#L284).

`close_campaign` closes only the `Campaign` account (reclaiming rent); the per-customer `CampaignProgress` PDAs — seeded `["cprogress", campaign, customer]` — are never closed. Because the campaign PDA is `["campaign", merchant, id]`, recreating the same `id` yields the **identical** address, so the old progress accounts silently become the progress for the new campaign. `earn_points_campaign` decides "fresh" solely from `progress.campaign == default`, which is false for the surviving account.

**Impact.** After any close+recreate of a campaign id:
- a customer who completed the old quest carries `completed = true` → the new quest pays `0` (silent reward denial);
- a customer with high stale `visits` instantly satisfies a shorter new quest target → unearned windfall of the new (possibly larger) reward;
- `bonus_drawn` pre-consumes the new `per_customer_cap`, and `participant_count` under-counts.

Merchant-scoped (their own points and stats), so Medium.

**Fix.** Include a monotonic `campaign_epoch`/creation-slot in the `CampaignProgress` seeds, or forbid id reuse, or validate `progress.campaign == campaign.key() && progress.created_for == campaign.created_at` before trusting a non-fresh progress account.

### L-1 — 🟡 Cooldown fails open under a backward clock

**Location:** [`execute.rs:188–195`](../programs/argus/src/instructions/execute.rs#L188).

`let elapsed = now.checked_sub(state.last_transfer_at).unwrap_or(i64::MAX);` — if `last_transfer_at > now` (validator clock moved backward), `elapsed` becomes `i64::MAX` and the cooldown is skipped. Not attacker-reachable under a monotonic clock (`last_transfer_at` is only ever written as `now`), but it resolves toward "allow," contrary to fail-closed.

**Fix.** `.unwrap_or(0)` so a future-dated timestamp *enforces* the cooldown.

### L-2 — 🟡 Global pause does not cover decay-rate / token-metadata mutation

**Location:** [`set_token_attribute.rs`](../programs/vesta-core/src/instructions/set_token_attribute.rs) — `handle_update_decay_rate`, `handle_update_token_metadata`, `handle_set_token_attribute`.

Earn, redeem, swap, clawback, grant, campaign, and register all begin with `require!(!config.paused, …)`. The three `SetTokenAttribute` handlers do not (the struct doesn't even take `config`). While the protocol is paused for an incident, the merchant owner key can still set the interest rate to the −100% cap or rewrite the token's on-chain name/symbol/uri.

**Impact.** The one class of value-/trust-affecting mutation that survives the kill-switch is exactly what an admin would pause to contain. Bounded to the pause window and that merchant's own token → Low.

**Fix.** Add the `config` account and `require!(!config.paused, …)` (and `!merchant.paused`) to the three handlers.

### L-3 — 🟡 Member inbound swap budget is self-set and unbounded

**Location:** [`koinon.rs:380–391` (`handle_set_swap_budget`)](../programs/vesta-core/src/instructions/koinon.rs#L380).

`set_swap_budget` requires only the `merchant_authority` signer (unlike `set_swap_rate`, which requires `merchant_authority` **and** `alliance_authority`) and imposes no ceiling. A member sets `swap_in_budget_raw = u64::MAX`, nullifying the "koinon risk boundary" that caps daily leg-B minting.

**Impact.** The alliance loses oversight of how much a member can be minted-into via swaps. No net value is created (every minted B is backed by burned A — see the conservation proof below), so impact is limited to that member's own supply inflation and loss of intended alliance governance.

**Fix.** Require the `alliance_authority` co-signer on `set_swap_budget` (mirror `set_swap_rate`) and/or clamp against an alliance-level maximum.

### L-4 — 🟡 `merchant.customer_count` under-counts

**Location:** [`earn_points.rs`](../programs/vesta-core/src/instructions/earn_points.rs) increments on first profile creation; [`offers.rs:200–204`](../programs/vesta-core/src/instructions/offers.rs#L200) and `clawback.rs:168–172` create the profile without incrementing.

A customer whose first interaction is `redeem_offer` (the documented gift-then-redeem path) or `clawback` gets a `CustomerProfile` with `wallet` set but `customer_count` untouched; a later `earn_points` then also skips the increment (`wallet != default`). Such customers are never counted.

**Fix.** Factor first-touch profile initialization into one helper that increments `customer_count`, and call it from the redeem and clawback paths.

### L-5 — 🟡 Quest can be structurally impossible to complete

**Location:** [`earn_points.rs:321–336`](../programs/vesta-core/src/instructions/earn_points.rs#L321).

Completion requires `bonus == gross_bonus` after clamping. If `per_customer_cap` (or remaining budget) is configured below `quest_reward`, the payout is always clamped, so `quest_completed` never becomes true — the customer is paid the capped bonus but `progress.completed` and `campaigns_completed` never advance. The "don't burn completion on a clamp" behavior is intentional for transient budget exhaustion, but a structural `cap < reward` makes the quest permanently incompletable.

**Fix.** Reject `per_customer_cap`/budget configs smaller than `quest_reward` at create/update time, or decide completion on quest-target attainment independent of the clamp.

### I-1 — 🔵 Stale `update_attestation` doc comment

**Location:** [`aegis/src/lib.rs:77`](../programs/aegis/src/lib.rs#L77). The comment says "also clearing revocation," but the handler rejects revoked attestations (`AlreadyRevoked`, terminal revocation). The code is the safe version; the comment invites a future maintainer to "fix" it toward unsafe behavior. Correct the comment.

### I-2 — 🔵 `verify_merchant` target unconstrained

**Location:** [`merchant_admin.rs:101–114`](../programs/vesta-core/src/instructions/merchant_admin.rs#L101). The `merchant` account is a bare `#[account(mut)]` with no seed/`has_one` binding. Safe today (admin-gated via `config` has_one; it only flips a bool), but it is the one privileged instruction whose target is not PDA-re-derived. Add seed derivation for defense in depth.

### I-3 — 🔵 UI-amount rounding wobble in swaps

Leg A credits the requested `ui_amount` while burning `floor(uiToRaw_A(ui_amount))`, so on a decayed mint the burned UI value can be ~0.01 UI-A less than credited, amplified by `ra/rb` into leg B. Attempted weaponization fails: the amplification reverses on an A→B→A round-trip (rates cancel) and leg-B output floors down, so the residual is sub-cent dust per swap while each swap burns real counter-brand value — no compounding profit path. Observation only. To close fully, compute leg B from the *actual* burned UI value (`raw_to_ui_A(raw_in)`) rather than the requested `ui_amount`.

---

## Verified correct (adversarially checked, no finding)

- **Cross-mint swap value conservation.** With `ra/rb` the members' alliance rates and `f` the fee, measuring both legs in the common alliance unit: `value_minted = rawToUi_B(raw_out)·rb ≤ ui_out·rb ≤ ui_amount·ra·(1 − f/10000) ≤ ui_amount·ra = value_burned`. Every rounding step floors the output; the fee only subtracts. Holds regardless of decimals or decay asymmetry; triangular cycles return ≤ start.
- **Swap account graph** fully PDA-bound: both members are PDAs under the *same* alliance, mints and ATAs bind to the same merchants, and leg-B mint authority is reconstructed from the program-owned merchant account. Self-dealing (own alliance + own merchants) is a closed loop with no external value sink.
- **Token-2022 extension authorities** in both mint-creation flows resolve to the program-controlled `merchant` PDA (or the pinned `ARGUS_ID`) — MetadataPointer, InterestBearingConfig (rate bounded `−10_000..=0`), TransferHook, PermanentDelegate, MintCloseAuthority. No caller-supplied authority; no backdoor. Donation-resistant PDA creation cannot be hijacked.
- **Achievement eligibility** is checked on-chain against the seed-bound `CustomerProfile`; the double-grant guard (`KleosReceipt`, no close, badge mint has no close authority) is burn-proof across achievement close/re-create.
- **argus PDA pinning / fail-closed.** Substituting a permissive `guard_config`, spoofing an attestation (self-made account or foreign issuer), or forging a list entry all fail closed. `guard_config` is `Account<GuardConfig>` pinned to the transferred `mint`; attestation checks program owner + pinned issuer + PDA derivation + in-data issuer/subject/schema/mask/revoked/time-window; missing wallet-state fails `StateNotOpened`.
- **argus↔aegis boundary.** The "attacker runs their own issuer" attack is defeated by three independent pins (program id, `attestation_issuer`, PDA derivation). The byte-offset table argus uses to read aegis attestations matches the aegis layout exactly.
- **Clawback** authority (owner/operator), delegate signing (merchant PDA = permanent delegate), source/destination pinning (this merchant's mint + named customer → treasury), daily cap with UTC rollover, mandatory reason code, and `init_if_needed` profile safety are all correct.
- **Two-step authority handovers** (config, alliance, issuer, argus guard) cannot be hijacked (`pending == Some(signer)`; null pending unacceptable) or front-run; `accept_issuer_authority` also clears the operator.
- **aegis** issue/update/revoke/close bind to the issuer via `has_one` + issuer-keyed seeds and gate on authority/operator — no cross-issuer forgery; expiry cannot be set in the past; pause is honored on issue/update and deliberately not on revoke/close.
- **PDA map:** all seed prefixes are distinct within each program and namespaced by program id across programs — no cross-type or cross-program collision.
- **Arithmetic:** `unsafe_code = forbid` and `clippy::arithmetic_side_effects = deny` are enforced at the workspace level; the logic-level review found no truncating cast or user-favoring rounding beyond the sub-cent wobble in I-3.

## Accepted / carried-forward risks (by design or pending mainnet)

These are known and documented; several are gated behind the mainnet checklist rather than fixed here.

- **No external audit.** This review is not a substitute (see the banner above).
- **Single-key admin & single-key program-upgrade authority** — move both to a multisig (Squads) + timelock before mainnet.
- **`init_config` is permissionless first-caller-wins** — gate to the upgrade authority or init atomically post-deploy.
- **`finalize_transfer_guard` is irreversible per mint** — a latent argus bug becomes permanent for finalized mints; a deliberate immutability tradeoff to weigh before finalizing on mainnet.
- **`migrate_config`** is inert dead code on any v2 deployment (`data_len == 42` never holds) — strip or feature-gate before mainnet.
- **Swap bypasses `MAX_EARN_PER_TX`** — bounded by the member's inbound budget; value-conserving, so accepted (see L-3 for the budget-governance gap).
- **`fee_bps` collects nothing** — the spread is a monetization stub (no alliance treasury).
- **Token metadata / decay mutability, no achievement close, unbounded `additional_metadata` growth** — product/lifecycle gaps, not security defects.

## Methodology

Five independent reviewers were run in parallel, each scoped to a program/concern with the vulnerability taxonomy above: (1) points economics, (2) koinon + clawback value integrity, (3) merchant/token/extension authorities, (4) the argus transfer hook, (5) aegis + the cross-program boundary + a protocol-wide PDA/signer sweep. Every finding a reviewer returned was then re-verified line-by-line against source by the coordinating reviewer before inclusion; claims without a concrete, source-backed exploit path were dropped. Findings already recorded in [`PRODUCTION_REVIEW.md`](PRODUCTION_REVIEW.md) (the v1 pass) were confirmed as fixed or carried forward, not re-counted.

## Remediation priority

1. **H-1** — bind `execute` to a real transfer (`transferring` flag + source-mint/owner checks). Single most important change.
2. **M-1** — arm the clawback cap by default; **M-3** — epoch-scope `CampaignProgress` seeds.
3. **M-2** — merchant authority rotation (design decision required).
4. **L-1/L-2/L-3** — quick, self-contained hardening.
5. **L-4/L-5, I-1/I-2** — data-integrity and documentation cleanups.

---

*Prepared by an AI-assisted adversarial review process. Maintainer: [ivasik-k7](https://github.com/ivasik-k7) · security contact `kovtun.ivan@proton.me`.*
