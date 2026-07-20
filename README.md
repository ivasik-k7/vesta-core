# VESTA — Living Loyalty Protocol on Solana

Loyalty points that behave like Vesta's sacred flame: they stay alive while the customer keeps engaging and cool down when left untended. Built for the "On-Chain Loyalty Rewards System" challenge.

**Live client:** https://dev-vesta.netlify.app/ · **Devnet:** [vesta_core](https://explorer.solana.com/address/gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz?cluster=devnet) · [argus](https://explorer.solana.com/address/9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx?cluster=devnet)

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
- argus IDL published on-chain; vesta_core IDL committed under [`idl/`](idl/) (SDK-canonical)

### Live demo transactions

Every mechanic, run end-to-end on devnet by [`scripts/seed-demo.ts`](scripts/seed-demo.ts)
(café **Kavarna** + bookstore **Litera** in one **Koinon** alliance, one demo customer).
The gift-over-cap **rejection** is the differentiator — the guard refusing a real transfer:

| Mechanic | Transaction |
|---|---|
| Register merchant (Token-2022 mint: metadata + decay + hook + delegate) | [Kavarna](https://explorer.solana.com/tx/5AW9ZVgsHCWoYBN3ykJn9p6sQkBBRh4f1nUzvq8LMu54MP4fTpnahMpvkNnKnjC9AxcC1dJXTsAE7LPJsNqNif8W?cluster=devnet) · [Litera](https://explorer.solana.com/tx/4KUMHBpiez8oCaYa7NSwnGrL5VvWnjqz5RgbhaDYyZYytQFR4R3jGc9127ot6K3ZeYiEDbFe9hRwtAHzb4tGt8RQ?cluster=devnet) |
| Initialize transfer guard (argus EAML) | [Kavarna](https://explorer.solana.com/tx/2b8iR6tVMw2azTycg3scrYFhFAQnSG6uJsqgkbqSCaHs74myxy5TsTn9txWzbHHpJh4aB9nddwBbiV9BzSH4xyCX?cluster=devnet) · [Litera](https://explorer.solana.com/tx/5UZaPobGHmiUMKVmpFFX1bGemNxDuwvPJFLVcmByafmUAKNhFRiRZB6sFfQptGABgiGFX5kwNdQxa4t3reFXqDsf?cluster=devnet) |
| Finalize guard (hook authority burned) | [Kavarna](https://explorer.solana.com/tx/35MZ1HFEmd4RTscEWyAKkLYnYc3kptW6neBgzbHjcSEQMBD3XZQSkcgD9ZGHuNzThYsYvva4WPow2D8bYKuLWxvL?cluster=devnet) |
| Earn points (merchant-signed, customer gasless, streak-boosted) | [Kavarna](https://explorer.solana.com/tx/4To7xWA7CZcfqdkG3REcLBaWqtRVLVQgPt3nzyqbAE5DLGUngaZZQhuHeLWo2YMdu7g4f1XpNYyE6Tgm5nmGRwzS?cluster=devnet) · [Litera](https://explorer.solana.com/tx/4eSvaDuxaRVXbBQ5dQkFJdp474c1NTE3M8SZ8xebgfwcRBV5BjuDMLmnYxmCmn5YadXWKybF99vLk7PtbirBCkWu?cluster=devnet) |
| Gift within cap (hooked transfer, 300 pts) | [tx](https://explorer.solana.com/tx/5gBE7KAho23R7n4fwgx4cDn1tApjPCxPcNYa5ojT1EBJbCCvDS5rNUYZbCWyEjc4o95Z7mqxZeEg5T5osqZF4Fju?cluster=devnet) |
| **Gift over cap → rejected by argus** (`GiftCapExceeded`) | enforced live; reproduce by re-running past the daily cap |
| Create alliance (Koinon) | [tx](https://explorer.solana.com/tx/4u5cCkvwmjN565z1kh38zZz9zAwhf6rQBm2nxZzg6dmxReHUmQr4SGDFnBikmg2SKwz7dqhWVC7AqD91Es3BjoEr?cluster=devnet) |
| Join alliance (handshake co-sign) | [Kavarna](https://explorer.solana.com/tx/34QJe4TXtES6ueENe3jSpB59BvkH9iXD1cqNUGs5ct6TGdog8mjBdy2QA3sp1FNKqEWKfTmF8moZDaiKtC9UMLhz?cluster=devnet) · [Litera](https://explorer.solana.com/tx/bc1XActzPxhNM7dkpMT8NzYE36zQLErZVFxC3YCRh5ef9voiQbb6AEFD4kqT4HZjPjeU3h5gmy1HjNu9ZqRY4Kq?cluster=devnet) |
| Swap points (Kavarna → Litera, UI-denominated) | [tx](https://explorer.solana.com/tx/4Nn5PymTqG17kGW456ZkGS7fApaJ72qN8c1N1G5k3i6iHvNsEeNFxuWGBuuDqWcBsN6qGFAVsrgrri74L3nhGs1?cluster=devnet) |
| Create offer | [tx](https://explorer.solana.com/tx/4XYHY78UWxtCFzYTemqHWHaE2Ns2LMvokxvUtPYdqPMxZUDUCwh7526kBh3jqd9JeWYCwszTc1vpTYs7Bv7ADu6a?cluster=devnet) |
| Redeem offer (on-chain UI→raw conversion + burn) | [tx](https://explorer.solana.com/tx/M75qxoEYZLnjLnzxPoVrDBxPE2pnFgR2VhY1oxq35nkF5u2SGU9jQ4bNDwrZZzhMGRCXtFUbrrid9oBdyeqvBAv?cluster=devnet) |

Demo actors: Kavarna `BGGZRPCrX7NjA77ywEpUCDLhBhae89Vaz6fqZcVAV4AB` · Litera `BrxTJzr1b9Ge75ZAyer1ZMBB8LkP6uNwdFNh91eqYALN` · customer `AdcZSvdJM4qY8Szi7uu4s4mtMFAwkWWJrszbhNz7becX`.

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
