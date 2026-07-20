# VESTA — Living Loyalty Protocol

**Enterprise-grade loyalty infrastructure on Solana.** Points that behave like a living flame: they reward engagement, cool down when untended, move only under merchant-defined policy, and compose across brands.

**Live client:** <https://dev-vesta.netlify.app/> · **Ecosystem:** [`vesta-ui`](https://github.com/ivasik-k7/vesta-ui) (web client) · [`vesta-sdk`](https://github.com/ivasik-k7/vesta-sdk) (Python, backend integrators)

---

## Table of contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Deployments](#deployments)
- [Core concepts](#core-concepts)
- [Account model](#account-model)
- [Instruction surface](#instruction-surface)
- [Security](#security)
- [Getting started](#getting-started)
- [Testing](#testing)
- [Operations runbook](#operations-runbook)
- [Live demo evidence](#live-demo-evidence)
- [Repository layout](#repository-layout)
- [Roadmap](#roadmap)
- [Maintainer](#maintainer)

---

## Overview

Traditional loyalty programs are siloed, restrictive, and static. VESTA turns loyalty points into a **living, composable on-chain primitive**:

| Capability | Mechanism |
|---|---|
| **Breathing points** | Token-2022 `InterestBearingConfig` with a negative rate — value cools over time; activity streaks outpace the decay |
| **Guarded transfers** | An SPL transfer hook (**argus**) enforces the token's own policy wherever it travels — fail-closed |
| **Cross-brand alliances** (*koinon*) | Merchants form on-chain alliances; customers atomically swap one brand's points for another's at governed rates |
| **Soulbound achievements** (*kleos*) | Non-transferable Token-2022 badges any external dApp can token-gate on |
| **Verifiable identity** (*aegis*) | An attestation registry guards can gate transfers on (region, KYC tier, age band) |
| **Accountable clawback** | `PermanentDelegate` recovery that is reason-coded, bounded by a public self-limit, and fully audited on-chain |

Every rule is enforced **by the programs**, not the client. The UI proposes; the chain disposes.

## Architecture

```
                        ┌──────────────────────┐
                        │      vesta_core      │  protocol: merchants, points,
                        │  gaMq6BpH…RG6L4LDz   │  offers, campaigns, badges,
                        └─────────┬────────────┘  alliances, clawback
                                  │ configures / finalizes
                 Token-2022 mint  │  (TransferHook extension)
                        ┌─────────▼────────────┐
   every peer transfer  │        argus         │  policy engine: velocity caps,
  ──────────────────────►  9zJEWrk4…Czsz3rx    │  allow/deny lists, cooldowns,
                        └─────────┬────────────┘  attestation gating — fail-closed
                                  │ reads (pinned PDA derivation)
                        ┌─────────▼────────────┐
                        │        aegis         │  attestation issuers: region,
                        │  AcCdMQC1…Thsu15e1   │  KYC tier, age band
                        └──────────────────────┘
```

- **vesta_core** owns the economy. Registering a merchant mints a Token-2022 point token in one transaction with: on-chain metadata, negative interest (decay), a transfer-hook extension pointed at argus, a permanent delegate for clawback, and a mint-close authority for clean deletion.
- **argus** is invoked by Token-2022 on **every** peer transfer. It resolves its policy account from pinned seeds (no account substitution possible), applies the policy, and rejects on any inconsistency. Deep spec: [`docs/ARGUS_SPEC.md`](docs/ARGUS_SPEC.md).
- **aegis** is a standalone attestation registry. argus derives attestation PDAs from a **pinned program ID and issuer**, so a malicious client cannot smuggle in a forged attestation.

## Deployments

| Program | Devnet ID | Metadata |
|---|---|---|
| `vesta_core` | [`gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz`](https://explorer.solana.com/address/gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz?cluster=devnet) | name/logo/security.txt/IDL published on-chain |
| `argus` | [`9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx`](https://explorer.solana.com/address/9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx?cluster=devnet) | ditto |
| `aegis` | [`AcCdMQC1rj4KukjhFzf4S8metEAXpnt9gzvMThsu15e1`](https://explorer.solana.com/address/AcCdMQC1rj4KukjhFzf4S8metEAXpnt9gzvMThsu15e1?cluster=devnet) | ditto |

Config PDA: `4aeV5JNqBXBa1M1gxch7b2h36hHBoobAR8Ajqax6J5Nr`. Canonical IDLs are committed under [`idl/`](idl/).

| Environment | Status |
|---|---|
| **Devnet** | ✅ Live (v2) — plus a seeded production-shaped demo dataset |
| **Mainnet** | ⏳ Planned — gated on external audit and multisig custody ([Roadmap](#roadmap)) |

## Core concepts

### Points lifecycle
1. **Earn** — the merchant (or an authorized operator) signs `earn_points`; the customer pays no gas. Streaks add +2%/day (jointly capped with campaigns at ×2.4); a single issue is capped at 1,000,000 raw units.
2. **Hold** — balances cool continuously via the mint's interest-bearing config; the UI value is computed by the token program itself (`amount_to_ui_amount`), so no client can misprice decay.
3. **Spend** — burn for offers (priced in decayed UI value), gift within guard policy, or swap cross-brand through an alliance (UI-value denominated on both legs, so mint age never leaks an edge).

### Multi-record ownership
Every major resource is keyed `(authority, id)` — one wallet may own many merchants, alliances, and issuers. `Merchant.id` leads the account layout so argus reads fixed byte offsets with no deserialization drift.

### Fail-closed guarding
argus rejects a transfer when *anything* is off: missing wallet-state, wrong PDA, paused guard, exceeded velocity, absent/revoked/expired attestation. There is no permissive fallback.

## Account model

| Account | Seeds | Program |
|---|---|---|
| `Config` | `["config"]` | vesta_core |
| `Merchant` | `["merchant", authority, id_le]` | vesta_core |
| point mint | `["mint", merchant]` | vesta_core |
| `CustomerProfile` | `["customer", merchant, wallet]` | vesta_core |
| `Offer` / `Campaign` / `Achievement` | `["offer" \| "campaign" \| "achieve", merchant, id_le]` | vesta_core |
| `CampaignProgress` | `["cprogress", campaign, customer]` | vesta_core |
| badge mint / `KleosReceipt` | `["badge" \| "kleos", achievement, customer]` | vesta_core |
| `Alliance` / `AllianceMember` | `["alliance", creator, id_le]` / `["member", alliance, merchant]` | vesta_core |
| `GuardConfig` / `WalletState` / `ListEntry` | `["guard", mint]` / `["wstate", mint, owner]` / `["entry", mint, target]` | argus |
| `Issuer` / `Attestation` | `["issuer", authority, id_le]` / `["attestation", issuer, subject]` | aegis |

## Instruction surface

<details>
<summary><b>vesta_core</b></summary>

- **Protocol admin:** `init_config`, `migrate_config`, `set_paused` (circuit breaker; transfers keep working so clawback is never bricked), `set_admin` / `accept_admin` (two-step), `verify_merchant`
- **Merchant lifecycle:** `register_merchant`, `close_merchant` (zero-supply only), `update_merchant`, `update_merchant_profile`, `set_merchant_paused`, `set_merchant_operator` (≤4 hot keys), `set_clawback_cap`
- **Token:** `update_decay_rate`, `update_token_metadata`, `set_token_attribute`, `finalize_transfer_guard` (permanently burns the hook authority)
- **Points:** `earn_points`, `earn_points_campaign`
- **Offers:** `create_offer`, `close_offer`, `redeem_offer`, `close_receipt`
- **Campaigns:** `create_campaign` (multiplier / flat bonus / quest; budgets, per-customer caps, tier & spend gates), `update_campaign` (extend / budget / pause), `close_campaign`
- **Achievements:** `create_achievement`, `grant_achievement` (permissionless — the chain checks eligibility), `close_achievement`
- **Alliances:** `create_alliance`, `join_alliance` (co-signed handshake), `leave_alliance`, `set_swap_rate`, `set_swap_budget`, `swap_points`, `set_alliance_params`, `set_alliance_paused`, `set_member_active`, two-step authority transfer
- **Clawback:** `clawback` (reason-coded, daily-capped, argus-audited)
</details>

<details>
<summary><b>argus</b></summary>

`initialize_transfer_guard`, `configure_policy` (gift/per-tx/balance caps, transfers-per-day, cooldowns, flags, attestation gates), `set_guard_paused`, `open_wallet_state`, `add_list_entry` / `remove_list_entry`, two-step authority transfer, `execute` (the hook).
</details>

<details>
<summary><b>aegis</b></summary>

`init_issuer`, `set_issuer_operator`, `set_issuer_paused`, `issue_attestation`, `update_attestation`, `revoke_attestation`, two-step authority transfer.
</details>

## Security

- **Reviews:** internal production review with 21 tracked findings — Tier 1–3 remediated ([`docs/PRODUCTION_REVIEW.md`](docs/PRODUCTION_REVIEW.md)). External audit pending before mainnet.
- **Disclosure:** [`SECURITY.md`](SECURITY.md) plus embedded on-chain `security.txt` in every program. Contact: `kovtun.ivan@proton.me`.
- **Design invariants:** fail-closed hook; pinned cross-program PDA derivation; two-step authority transfers throughout; owner/operator privilege separation; public self-limits on the most sensitive power (clawback).
- **Known limitations:** single-key admin on devnet (multisig planned for mainnet); `init_config` is first-come (deploy-time gate tracked in the review).

## Getting started

### Prerequisites

Rust 1.89 (pinned via `rust-toolchain.toml`) · Solana CLI 4.1.x (Agave) · Anchor 1.1.2 · Node.js ≥ 20 · a funded devnet keypair at `~/.config/solana/id.json`.

### Build & verify

```bash
npm install
anchor build                                    # all three programs
cargo test                                      # LiteSVM integration suite (52 tests)
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
```

### Deploy (devnet)

```bash
anchor deploy --provider.cluster devnet
npx tsx scripts/init-config-devnet.ts           # one-time Config PDA
# program metadata / security.txt / logos / IDLs: see metadata/README.md
```

### Seed a production-shaped demo

```bash
RPC_URL="https://devnet.helius-rpc.com/?api-key=…" npm run seed
```

Idempotent. Provisions 5 merchants (guards, offers, campaigns, achievements), a 3-member alliance, an aegis issuer with attestations, points across ~8 customers, a redemption, a guarded gift, and a clawback — then verifies every account and prints an explorer-linked report. Ephemeral customer keys persist to `scripts/.seed-state.json` (gitignored).

## Testing

| Layer | Tooling | Coverage |
|---|---|---|
| Unit | `cargo test` | math, state helpers |
| Integration | **LiteSVM** (bundled Token-2022) | 52 tests: happy paths, guard rejections, cap violations, authority checks, campaign kinds, alliance swaps, clawback accounting |
| Live | `scripts/seed-master.ts` | end-to-end devnet run with per-step ✓/✗ verification |

## Operations runbook

- **RPC:** the public devnet endpoint rate-limits `getProgramAccounts`; run against a private endpoint (e.g. Helius) via `RPC_URL`.
- **Upgrades:** `solana program extend <id> <bytes>` before deploying a larger binary (minimum 10,240 bytes).
- **IDLs:** large IDLs publish via URL pointer through `@solana-program/program-metadata`; small ones go on-chain directly.
- **Observability:** all state is chain-derivable; the UI's Network / Analytics pages double as a live health view.

## Live demo evidence

<details>
<summary>Every mechanic executed on devnet (café <b>Kavarna</b> + bookstore <b>Litera</b> in one <b>Koinon</b> alliance)</summary>

| Mechanic | Transaction |
|---|---|
| Register merchant (Token-2022 mint: metadata + decay + hook + delegate) | [Kavarna](https://explorer.solana.com/tx/5AW9ZVgsHCWoYBN3ykJn9p6sQkBBRh4f1nUzvq8LMu54MP4fTpnahMpvkNnKnjC9AxcC1dJXTsAE7LPJsNqNif8W?cluster=devnet) · [Litera](https://explorer.solana.com/tx/4KUMHBpiez8oCaYa7NSwnGrL5VvWnjqz5RgbhaDYyZYytQFR4R3jGc9127ot6K3ZeYiEDbFe9hRwtAHzb4tGt8RQ?cluster=devnet) |
| Initialize transfer guard (argus EAML) | [Kavarna](https://explorer.solana.com/tx/2b8iR6tVMw2azTycg3scrYFhFAQnSG6uJsqgkbqSCaHs74myxy5TsTn9txWzbHHpJh4aB9nddwBbiV9BzSH4xyCX?cluster=devnet) · [Litera](https://explorer.solana.com/tx/5UZaPobGHmiUMKVmpFFX1bGemNxDuwvPJFLVcmByafmUAKNhFRiRZB6sFfQptGABgiGFX5kwNdQxa4t3reFXqDsf?cluster=devnet) |
| Finalize guard (hook authority burned) | [Kavarna](https://explorer.solana.com/tx/35MZ1HFEmd4RTscEWyAKkLYnYc3kptW6neBgzbHjcSEQMBD3XZQSkcgD9ZGHuNzThYsYvva4WPow2D8bYKuLWxvL?cluster=devnet) |
| Earn points (merchant-signed, customer gasless, streak-boosted) | [Kavarna](https://explorer.solana.com/tx/4To7xWA7CZcfqdkG3REcLBaWqtRVLVQgPt3nzyqbAE5DLGUngaZZQhuHeLWo2YMdu7g4f1XpNYyE6Tgm5nmGRwzS?cluster=devnet) · [Litera](https://explorer.solana.com/tx/4eSvaDuxaRVXbBQ5dQkFJdp474c1NTE3M8SZ8xebgfwcRBV5BjuDMLmnYxmCmn5YadXWKybF99vLk7PtbirBCkWu?cluster=devnet) |
| Gift within cap (hooked transfer) | [tx](https://explorer.solana.com/tx/5gBE7KAho23R7n4fwgx4cDn1tApjPCxPcNYa5ojT1EBJbCCvDS5rNUYZbCWyEjc4o95Z7mqxZeEg5T5osqZF4Fju?cluster=devnet) |
| **Gift over cap → rejected by argus** (`GiftCapExceeded`) | enforced live; reproduce by exceeding the daily cap |
| Create alliance (Koinon) | [tx](https://explorer.solana.com/tx/4u5cCkvwmjN565z1kh38zZz9zAwhf6rQBm2nxZzg6dmxReHUmQr4SGDFnBikmg2SKwz7dqhWVC7AqD91Es3BjoEr?cluster=devnet) |
| Join alliance (handshake co-sign) | [Kavarna](https://explorer.solana.com/tx/34QJe4TXtES6ueENe3jSpB59BvkH9iXD1cqNUGs5ct6TGdog8mjBdy2QA3sp1FNKqEWKfTmF8moZDaiKtC9UMLhz?cluster=devnet) · [Litera](https://explorer.solana.com/tx/bc1XActzPxhNM7dkpMT8NzYE36zQLErZVFxC3YCRh5ef9voiQbb6AEFD4kqT4HZjPjeU3h5gmy1HjNu9ZqRY4Kq?cluster=devnet) |
| Swap points (UI-denominated, cross-brand) | [tx](https://explorer.solana.com/tx/4Nn5PymTqG17kGW456ZkGS7fApaJ72qN8c1N1G5k3i6iHvNsEeNFxuWGBuuDqWcBsN6qGFAVsrgrri74L3nhGs1?cluster=devnet) |
| Create offer | [tx](https://explorer.solana.com/tx/4XYHY78UWxtCFzYTemqHWHaE2Ns2LMvokxvUtPYdqPMxZUDUCwh7526kBh3jqd9JeWYCwszTc1vpTYs7Bv7ADu6a?cluster=devnet) |
| Redeem offer (on-chain UI→raw conversion + burn) | [tx](https://explorer.solana.com/tx/M75qxoEYZLnjLnzxPoVrDBxPE2pnFgR2VhY1oxq35nkF5u2SGU9jQ4bNDwrZZzhMGRCXtFUbrrid9oBdyeqvBAv?cluster=devnet) |

A larger production-shaped dataset (5 merchants, attestations, clawback) is provisioned by `scripts/seed-master.ts`.
</details>

## Repository layout

```
programs/
  vesta-core/        # protocol program (Rust / Anchor)
  argus/             # transfer-hook policy engine
  aegis/             # attestation registry
docs/
  ARGUS_SPEC.md      # deep technical spec of the hook engine
  PRODUCTION_REVIEW.md
idl/                 # canonical IDLs (also published on-chain)
metadata/            # logos, metadata.json, security.json + publish runbook
scripts/
  seed-master.ts     # idempotent production-shaped demo provisioning
  earn-to-me.ts      # issue any brand's points to any wallet (demo helper)
  init-config-devnet.ts
tests/               # LiteSVM integration suite
SECURITY.md
```

## Roadmap

- [ ] External security audit
- [ ] Mainnet deployment behind a Squads multisig; role separation (admin ≠ deployer ≠ demo)
- [ ] `init_config` deploy-time gate
- [ ] Offer time-windows · achievement metadata updates · attribute removal
- [ ] Indexer (Helius webhooks / DAS) for protocol-scale analytics
- [ ] Python SDK GA for backend integrators

## Maintainer

Built and maintained by [**ivasik-k7**](https://github.com/ivasik-k7). Security contact: `kovtun.ivan@proton.me`.

> VESTA is deployed on **devnet** for evaluation. It is not financial infrastructure until audited and released on mainnet.
