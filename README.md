# VESTA — Living Loyalty Protocol on Solana

Loyalty points that behave like Vesta's sacred flame: they stay alive while the customer keeps engaging and cool down when left untended. Built for the "On-Chain Loyalty Rewards System" challenge.

## Why

Traditional loyalty programs are siloed, restrictive, and static. VESTA turns points into a living, composable primitive:

- **Breathing points** — Token-2022 mints with a negative `InterestBearingConfig` rate: value decays over time, activity streaks earn multipliers that outpace the decay.
- **Cross-merchant alliances (koinon)** — merchants form on-chain alliances; customers atomically swap one brand's points for another's at alliance-governed rates.
- **Soulbound achievements (kleos)** — non-transferable Token-2022 badges that any external dApp can token-gate on.
- **Guarded transfers (argus)** — a transfer hook program enforces the token's own rules wherever it travels: gifting within limits, no mercenary dumping.

## Workspace

| Program | Purpose |
|---|---|
| [`programs/vesta-core`](programs/vesta-core) | Merchants, campaigns, earn/redeem, customer profiles, alliances, badges |
| [`programs/argus`](programs/argus) | SPL transfer hook: validates every point transfer (phase 2) |

Program IDs (devnet):

- `vesta_core`: `Am2X4B1SCnJKXL8Yir2j6yGpHAKrmwcf2E5aKnA9BZV`
- `argus`: `CrzLCMSQ1pWTuLXBomoLn6eAB1c1gLsw5x9sBeuyBNKt`

## Development

Toolchain: Rust 1.89 (pinned via `rust-toolchain.toml`) · Solana CLI 4.1.1 (Agave) · Anchor 1.1.2.

```bash
anchor build   # compile both programs
cargo test     # LiteSVM integration tests
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
```

`scripts/spike-token2022.ts` is the phase-0 spike proving the full extension stack (metadata, interest-bearing decay, transfer hook, permanent delegate, non-transferable badges) composes on one mint:

```bash
npm install
RPC_URL=http://127.0.0.1:8899 npm run spike   # or omit RPC_URL for devnet
```

## Ecosystem

`vesta-program` (this repo, on-chain) · `vesta-sdk` (Python client for merchant backends/integrators) · `vesta-ui` (React web client for customers and merchants).

## Status

Phase 0/1 — scaffold deployed, `init_config` implemented and tested. Architecture doc, tradeoffs, and devnet transaction links land here as the build progresses.
