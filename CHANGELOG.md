# Changelog

All notable changes to the VESTA on-chain programs are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [2.0.0] — 2026-07 (devnet)

Current live deployment. Program IDs in [README → Deployments](README.md#deployments).

### Added
- **vesta_core**: multi-record ownership `(authority, id)` for merchants,
  alliances, and issuers; offers, campaigns (multiplier / flat bonus / quest,
  budgets, per-customer caps, tier & spend gates), achievements with soulbound
  Token-2022 badges; alliances with governed UI-value swaps, member handshake,
  fee/rate-bound parameters; reason-coded clawback with public self-limit;
  operator keys (≤4); two-step admin transfer; merchant verification registry.
- **argus**: fail-closed transfer hook — velocity caps, cooldowns,
  allow/deny lists, gift caps, attestation gating; pinned PDA derivation;
  guard finalization (hook authority burn).
- **aegis**: attestation issuer registry (region / KYC tier / age band),
  issue / update / revoke, operator support.
- On-chain program metadata: name, logo, `security.txt`, IDL for all three
  programs; canonical IDLs committed under `idl/`.
- LiteSVM integration suite (52 tests); idempotent devnet seed
  (`scripts/seed-master.ts`) with per-step verification.

### Security
- Internal production review: 21 tracked findings, Tier 1–3 remediated
  (`docs/PRODUCTION_REVIEW.md`).
- Internal adversarial pre-audit (`docs/SECURITY_AUDIT.md`): 1 High, 3 Medium,
  5 Low, 3 Info; cross-mint swap proven value-conserving; Token-2022 extension
  authorities verified backdoor-free. Not a substitute for a third-party audit.
- Remediated 9/10 actionable audit findings (M-2 deferred to the mainnet
  migration): argus `execute` now fails closed outside a real transfer (H-1);
  clawback is owner-only (M-1); campaign progress is instance-scoped by slot
  (M-3); plus L-1..L-5 and I-1/I-2 hardening. Regression tests added for H-1
  and M-1.

## [1.0.0] — 2026-07 (devnet, superseded)

Initial protocol: merchant registration with Token-2022 point mints
(metadata, decay, transfer hook, permanent delegate), gasless earns with
streaks, offer redemption, basic guard policies.
