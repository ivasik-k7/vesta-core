# 05 · Decay-Reward Mechanics — "Ember / Pledge / Kinship"

> **Status:** Draft / Proposed · **Layer:** Differentiator (wave 2) · **Depends on:** 01 (vaults), 02 (treasury/governance), 04 (status weighting)
> Inherits all [shared conventions](README.md#shared-conventions-normative-for-all-specs).

## 1. Summary

Three mechanics that turn VESTA's most unique-but-underused primitive — **decay** —
from a resented stick into an opt-in game, stacking loss aversion with variable
reward and commitment psychology:

- **Ember** — a **decay dividend pool**: value that decays (and stakes that are
  forfeited) is recycled into a communal pool redistributed to *active* members'
  customers each epoch. "The customers who lapsed fund your reward."
- **Pledge** — **commitment term-vaults**: a customer locks points against a
  self-set goal; succeed and the stake returns with a bonus *and* immunity from
  the decay that elapsed; fail and the stake is forfeited into Ember.
- **Kinship** — **decay-vested referrals**: the referrer's bonus mints in a
  fast-decaying "vesting" state and only converts to permanent points as the
  referred friend proves real, `aegis`-verified activity — so Sybil farming melts
  by design.

## 2. The decay constraint (read first)

`InterestBearingConfig` decay is **virtual**: it lowers a balance's *UI value*
(`amount_to_ui_amount`) while the *raw* balance is unchanged. There is **no burn
event** to sweep. So "pool the decayed points" cannot mean intercepting a burn.
Instead these mechanics are funded **value-correctly** from real, capturable
sources (below), and every design here is explicit about where the value comes
from — no hand-waving, no unbacked mint (invariant #4).

## 3. Ember — Decay Dividend Pool

### 3.1 Funding sources (all real)
1. **Forfeited pledges** (§4) — a customer who fails a commitment forfeits a
   staked UI value `V`; `V` is credited to the pool. *Real, primary source.*
2. **Decay-proxy accrual** — as a merchant's points decay, its outstanding **UI
   liability shrinks**. The merchant may mint fresh points equal to a governed
   fraction of that *decayed UI value* into the pool **without increasing net UI
   liability** — recycling decay into retention at zero marginal liability. The
   decayed UI value is measured from aggregate supply via `amount_to_ui_amount`
   deltas over the epoch (a crank samples it).
3. **Treasury allocation** — governance (spec 02) may top the pool up from the
   alliance treasury.

### 3.2 Distribution
An `EmberPool` PDA per alliance accrues from §3.1. On each **epoch** close, the
pool is split among eligible customers — those who passed an activity threshold
that epoch — weighted by **alliance status tier / streak** (spec 04). Payout is a
spec-01-style vault transfer/mint. Distribution is a **permissionless crank**
(`settle_ember_epoch`) computing a claim root; customers `claim_ember`.

### 3.3 Anti-Sybil (killer risk)
Eligibility requires real qualifying spend (min threshold) and, for a share
above a floor, an `aegis` attestation. Weighting by status (spec 04, itself
Sybil-resistant) means bots with no tier earn a negligible share.

## 4. Pledge — Commitment Term-Vaults

### 4.1 Mechanic
A customer opens a `Pledge`: lock `principal` points against a goal predicate
(e.g., "≥4 qualifying visits in 30 days", "spend ≥ X across the alliance",
"complete quest Q") with a deadline.

- **Deposit:** burn `principal`; record its **UI value `V`** at deposit slot
  (the commitment's real size, decay-normalized).
- **Success** (goal met by deadline): mint `V`-equivalent points back **plus a
  bonus** — so the customer is made whole against the decay that elapsed while
  locked (the "decay immunity" reward) plus upside. Bonus is bounded and funded
  from a spec-01 vault or the treasury; a completed-pledge streak writes a
  soulbound badge.
- **Failure / early exit:** the stake is **forfeited** — `V` is credited to the
  **Ember pool** (§3), enforced by the vault (optionally clawed via
  `PermanentDelegate`). No silent expiry: terms and the deposit slot are on-chain,
  so the customer can prove their exact deal.

### 4.2 Why on-chain only
A commitment device needs a **credible, automatic penalty**. Web2 can't escrow
and slash loyalty points trustlessly. VESTA's decay + `argus` fail-closed
enforcement make the forfeit automatic and believable — self-enforced retention,
a category no incumbent occupies.

### 4.3 Framing / regulatory (killer risk)
Keep the bonus **modest and utility-denominated** (points, not cash); frame as a
personal challenge with upside, cap stake sizes, and never let a locked,
"protected" balance read as a yield-bearing deposit. Gate longer terms behind
`aegis` where required.

## 5. Kinship — Decay-Vested Referrals

### 5.1 Mechanic
`create_referral(referrer, referee)` mints the referral bonus into a **vesting
bucket** (a `Referral` PDA-held balance) on an **accelerated decay schedule**.
Each qualifying, `aegis`-verified action by the referee converts a **tranche** to
permanent, decay-normal points for the referrer. If the referee never becomes
real, the bonus melts away.

- Referrer is paid for **durable acquisition**, not signups.
- `aegis` gates referee **uniqueness/region** (one attested identity), so a
  sock-puppet referee can't convert tranches.
- Cross-brand variant: a member may refer into another member (spreads customers
  across the coalition), the bonus split per governance.

### 5.2 Why on-chain only
Decay-as-vesting is native: the reward literally melts unless acquisition proves
real, making Sybil farming unprofitable **by construction** rather than via a
Web2 fraud team and manual clawbacks. Depends on `aegis` strength (killer risk):
weak identity → vesting slows but doesn't stop farming.

## 6. Account model

```
EmberPool        seeds = ["ember", alliance]                 // NEW
  alliance, reserve (PDA-owned token acct), accrued, epoch, epoch_ends_at, last_decay_sample
EmberClaim       seeds = ["ember-claim", alliance, epoch_le, customer]  // dedupe claims

Pledge           seeds = ["pledge", customer, pledge_id_le]  // NEW
  customer, alliance(optional), principal_ui, deposit_slot, goal: Predicate,
  deadline, bonus_bps, state (Open/Succeeded/Forfeited), bump

Referral         seeds = ["referral", referrer, referee]     // NEW
  referrer, referee, total_bonus, vested, tranche_size, decay_schedule,
  converted_tranches, bump

pledge-streak badge  seeds = ["pledge-badge", customer]      // NonTransferable
```

Governance-set (spec 02): `decay_proxy_bps`, `ember_epoch_secs`, activity
thresholds, `pledge_bonus_cap_bps`, `max_pledge_principal`, `referral_bonus`,
`referral_decay_schedule`.

## 7. Instruction surface

**Ember**
- `accrue_ember_decay_proxy()` — permissionless crank; samples aggregate decayed
  UI value since `last_decay_sample`, mints the governed fraction to the pool
  (value-neutral per §3.1.2).
- `settle_ember_epoch()` — permissionless crank after `epoch_ends_at`; computes
  eligible + weighted claim root; opens the next epoch.
- `claim_ember(epoch)` — customer claims their weighted share once
  (`EmberClaim` dedupe).
- `fund_ember_from_treasury(amount)` — via spec 02 `SpendTreasury` execution.

**Pledge**
- `open_pledge(principal, goal, deadline, ...)` — burns `principal`, records `V`
  and slot.
- `record_pledge_progress()` — updates goal progress from on-chain state
  (visits/spend/quest), pinned reads.
- `settle_pledge()` — at/after deadline: success → mint `V` + bonus (+ streak
  badge); failure/early → forfeit `V` to Ember. Permissionless crank at deadline;
  customer-signed for early exit.

**Kinship**
- `create_referral(referee)` — referrer opens the vesting bucket; referee must
  resolve to a fresh, aegis-attested identity.
- `convert_referral_tranche()` — on a qualifying aegis-verified referee action,
  converts one tranche to permanent points; permissionless crank verifying the
  referee's on-chain activity + attestation.

## 8. Math & limits

- **Decay-proxy accrual (§3.1.2):** `pool_in = decayed_ui_value_epoch ·
  decay_proxy_bps / 10_000` (floor). Bounded so net UI liability is
  non-increasing: `pool_in ≤ decayed_ui_value_epoch`. Sampled via
  `amount_to_ui_amount` on tracked supply; `checked_*`.
- **Ember split:** `share(c) = pool · weight(c) / Σ weight` (floor); dust retained
  to the next epoch. `weight` from status tier/streak (spec 04).
- **Pledge:** `V` = UI value at deposit (shared UI path); success mint = `V + V ·
  bonus_bps/10_000` (bonus capped by `pledge_bonus_cap_bps`, funded from a bounded
  vault); the success mint respects `MAX_EARN_PER_TX`.
- **Kinship:** `tranche = total_bonus / n_tranches`; vesting decay on the
  unconverted remainder uses the same in-program linear decay as spec 04's score.
- All floored toward the protocol; forfeits credited exactly (`Σ` conserved).

## 9. Security considerations

- **Value conservation (#4):** Ember only redistributes value it actually
  captured (forfeits + decay-proxy bounded by measured decay + treasury). Pledge
  success mint is bounded and vault/treasury-funded; failure mints nothing.
  Kinship mints the bonus once (into vesting) and only *converts* it — no
  double-mint.
- **Sybil (the cross-cutting killer risk):** every payout path leans on `aegis`
  identity + status weighting + min-spend thresholds; bots earn negligible pool
  share, can't convert referral tranches, and can't cheaply farm pledges.
- **Pledge forfeiture enforcement:** vault-held/burned principal; forfeit routed
  by the program, optionally `PermanentDelegate`-clawed; `argus` fail-closed on
  any locked-balance transfer.
- **Pinned derivation (#3), pause (L-2), owner/governance gating (M-1)** as
  standard. Ember cranks are permissionless but only *move already-captured*
  value per the deterministic root.
- **Regulatory:** Pledge "protected" balances and Ember "dividends" must stay
  utility-scoped, modest, and clearly not cash instruments (see §4.3); gate behind
  the mainnet legal review.

## 10. Migration & compatibility

- All new accounts; entirely additive. No change to existing earn/redeem/swap
  behavior unless a customer opts into a pledge/referral or an alliance enables
  Ember.
- Requires spec 01 (bonus/pool vaults), spec 02 (governed params, treasury
  top-up), spec 04 (status weighting for Ember). Ships **last**.

## 11. Test plan (LiteSVM)

- **Ember:** decay-proxy accrual is bounded by measured decayed value (never
  inflates net UI liability); epoch settle splits by weight; double-claim
  rejected; treasury top-up via governance.
- **Pledge:** success mints `V` + capped bonus + badge; failure/early forfeits
  exactly `V` to Ember; on-chain terms provable; stake locked (transfer blocked).
- **Kinship:** tranche converts only on aegis-verified referee activity; sock-
  puppet referee cannot convert; unconverted bonus decays; no double-mint.
- **Conservation:** across a full cycle, `Σ` value in = `Σ` value out + retained
  dust; no unbacked mint.

## 12. Phased rollout

1. **Pledge** — self-contained commitment vaults; forfeits create the first real
   Ember inflow. (Smallest, highest novelty.)
2. **Ember** — pool + epoch settle + claim, fed by pledge forfeits + treasury;
   add decay-proxy accrual once sampling is validated.
3. **Kinship** — referral vesting (needs strong aegis).

## 13. Open questions

- Decay-proxy sampling: track aggregate supply per mint (extra state) vs. sample
  on redemption. Deferred; Pledge + treasury fund Ember day one without it.
- Pledge principal: burn-and-remint (chosen, value-correct) vs. move to a
  non-decaying term-mint (extra mint, cleaner UX). Decide before phase 1.
- Ember epoch cadence and eligibility threshold — governance-tunable.
- Should a locked Pledge also pause spec 04 status decay ("locking defends
  status")? Attractive; evaluate with spec 04 §12.
