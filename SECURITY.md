# Security Policy

VESTA is a Solana loyalty protocol (`vesta_core`), its transfer-hook policy
engine (`argus`), and an attestation issuer (`aegis`). This document is the
security contact and disclosure policy referenced by each program's embedded
`security.txt` and on-chain program-metadata `security` account.

## Reporting a vulnerability

Email **kovtun.ivan@proton.me** with:

- the affected program (`vesta_core` / `argus` / `aegis`) and program id,
- a description and, ideally, a reproduction (a failing test or a devnet tx),
- the impact you believe it has.

Please **do not** open a public issue for anything exploitable. We aim to
acknowledge within 72 hours.

## Scope

In scope: the on-chain programs in this repository and their account/PDA
invariants — authority checks, PDA derivations, arithmetic, the transfer-hook
fail-closed guarantees, and the argus↔aegis attestation trust boundary.

Out of scope: the reference UI, off-chain metadata hosting, third-party RPC
providers, and phishing of end-user wallets.

## Deployments (devnet)

| Program      | Program id                                     |
| ------------ | ---------------------------------------------- |
| `vesta_core` | `gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz`  |
| `argus`      | `9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx` |
| `aegis`      | `AcCdMQC1rj4KukjhFzf4S8metEAXpnt9gzvMThsu15e1` |

This is a challenge/hackathon codebase on devnet; it has **not** been audited.
Do not use it with real value.

## Safe harbor

Good-faith research that respects this policy — no data destruction, no
privacy violations, no service degradation, only devnet — will not be pursued
as a violation of these terms.
