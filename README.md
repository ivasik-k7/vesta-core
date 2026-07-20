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

## Devnet

Deployed programs (all four phases live):

- [`vesta_core`: `gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz`](https://explorer.solana.com/address/gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz?cluster=devnet)
- [`argus`: `9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx`](https://explorer.solana.com/address/9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx?cluster=devnet)
- [`init_config`](https://explorer.solana.com/tx/G3CKMtqSkNx1F9GMPZtQiJxiwcSFG65gsVMHhK91QybWGucweMnLXX7PhrATdejyf9DdvPMisTge8S9Z3DM9urc?cluster=devnet) — Config PDA `4aeV5JNqBXBa1M1gxch7b2h36hHBoobAR8Ajqax6J5Nr`

Program ids were rotated when the full protocol shipped: the phase-0 deployment's
upgrade authority had already been handed to the owner wallet, so development
continues under a hot dev key on fresh ids; authority moves to the owner wallet
again at submission (spec §16). Historical phase-0 artifacts below.

Phase-0 deployment (historical):

- [`vesta_core`: `Am2X4B1SCnJKXL8Yir2j6yGpHAKrmwcf2E5aKnA9BZV`](https://explorer.solana.com/address/Am2X4B1SCnJKXL8Yir2j6yGpHAKrmwcf2E5aKnA9BZV?cluster=devnet) (IDL published on-chain)
- [`argus`: `CrzLCMSQ1pWTuLXBomoLn6eAB1c1gLsw5x9sBeuyBNKt`](https://explorer.solana.com/address/CrzLCMSQ1pWTuLXBomoLn6eAB1c1gLsw5x9sBeuyBNKt?cluster=devnet)

Live transactions:

- [`init_config`](https://explorer.solana.com/tx/2AafQDwvTa7UDVMo3DTPFH4f8yqyzevBAZP6wpjLdyttZmugQccoBRxheXsYXjyRxt2AwFUi9cmV8JNKULWoEp61?cluster=devnet) — protocol Config PDA (`6rP9Q1QLGFZBhGr4AJUqmbc3SnmVHyc8x2rqVw2jTexu`)
- Token-2022 extension spike, points mint `EsRwnbKnSWHb1D7BuMkTG1mvWwEFVZanfJ6uyuS6uDT2` (metadata + interest-bearing decay + transfer hook + permanent delegate): [create](https://explorer.solana.com/tx/axZqjspne6a5k2f8qeyA944B12cAcos8ZRJR58DDwc5uLPzAAstpgBLynDte7rMkcCtCKRid3qMcn9jaskLsLeR?cluster=devnet) · [mint](https://explorer.solana.com/tx/5WMsALjt6bTxPMY1w5EnbGftBGR1p31ihcjumY1SspQP1zfwnNUiZFCtKNvDUx25GGAXPobxwT8JXbeBzsj1qznB?cluster=devnet)
- Soulbound badge spike, mint `CK56tapA35nEKQCFvjPWaQfxqYMC5ZX7hf3SKMQX44t3` (non-transferable, supply frozen): [create](https://explorer.solana.com/tx/3eRUnC2nc57Rvq6fj5J1ecKo97WXr9VMF86M9Dh4pP78zimi2VyUQN6AFdi5iXFwe3eeYv5qdozw3CEg9t86pPEx?cluster=devnet)

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

`vesta-core` (this repo, on-chain) · [`vesta-sdk`](https://github.com/ivasik-k7/vesta-sdk) (Python client for merchant backends/integrators) · [`vesta-ui`](https://github.com/ivasik-k7/vesta-ui) (React web client for customers and merchants).

## Status

Phase 0/1 — scaffold deployed, `init_config` implemented and tested. Architecture doc, tradeoffs, and devnet transaction links land here as the build progresses.
