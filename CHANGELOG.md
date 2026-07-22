# Changelog

All notable changes to the VESTA on-chain programs are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased] — Identity & Trust rework (specs 06/07/09)

Crates bumped to **2.0.0** (breaking on-chain layout changes; devnet redeploy).

### Added — aegis accreditation trust graph ([spec 08](docs/specs/08-aegis-issuer-accreditation.md) phase 1a)
- `TrustRoot` (`register_trust_root`) + `Accreditation` (`accredit_issuer` /
  `revoke_accreditation`): a root vouches for an issuer to issue certain schemas
  in a jurisdiction. `verify_accreditation(root, issuer, schema)` returns a
  `Verdict` via return-data — so a verifier **pins one root** and any issuer it
  accredits inherits trust (no hardcoded issuer allowlist). Revocation
  de-trusts the issuer instantly. Recursive multi-hop chains, DID/real-world PKI
  (secp256r1), and bonds/cascade timelock are later sub-phases.

### Added — argus enforces aegis Policies ([spec 09](docs/specs/09-argus-policy-vm.md) phase 2)
- `GuardConfig.policy`: when set, `refresh_eligibility` CPIs aegis
  **`verify_policy`** instead of a raw `Present` check — so the compliance rule
  (jurisdiction / schema / freshness) lives in an aegis `Policy` as **data,
  editable with no argus redeploy**. `default()` keeps the legacy `Present` path.
- Rationale: this delivers the spec-09 "rule as data" thesis via the policy
  layer. The full data-driven mechanical-rule interpreter + dynamic
  ExtraAccountMetaList are **deferred** (low marginal value — caps/velocity are
  stable — and the highest breakage risk in the system).

### Added — aegis policy engine ([spec 07](docs/specs/07-aegis-verify-and-policy.md) phase 2)
- Named, versioned, **jurisdiction-tagged** `Policy` accounts (`register_policy`/
  `deprecate_policy`) with a **freshness** (max-credential-age) requirement.
- `verify_policy(subject)` returns a `Verdict` (return-data) and emits a
  reproducible `PolicyDecision` audit event stamped with the policy version.
- **Poseidon deferred** to the ZK phase: it requires BN254 field-element inputs
  (`InputLargerThanModulus`), adding complexity for no present (non-ZK) benefit;
  phase 1/2 use sha256 for commitments/Merkle. Devnet re-issue on migration is free.

### Changed — aegis (privacy-preserving attestations, [spec 06](docs/specs/06-aegis-commitment-substrate.md)/[07](docs/specs/07-aegis-verify-and-policy.md))
- `Attestation` no longer stores a public `value` bitmask. It stores a hiding
  **commitment** + per-attribute Merkle root; PII lives off-chain (GDPR-safe).
- **Multi-credential**: PDA re-seeded `["attestation", issuer, subject, schema_id]`.
- New **`Schema` registry** (`register_schema`/`deprecate_schema`), SAS-aliasable.
- New terminal **`erase_attestation`** (cryptographic erasure) + `status` enum.
- New **`verify(subject, predicate) → Verdict`** interface (return-data;
  `Present` + `AttributeDisclosed`/sha256 Merkle), the stable way to consume aegis.
- Every account carries a `version` header.

### Changed — argus (capability-based eligibility, [spec 09](docs/specs/09-argus-policy-vm.md))
- `execute` no longer reads aegis by fixed byte offset. It reads a cached
  **`EligibilityCapability`** (`["cap", mint, subject]`) — no hot-path CPI.
- New off-path **`refresh_eligibility`** CPIs aegis `verify` once and caches the
  verdict bitmap; `GuardConfig` gains `version`/`policy_epoch`/`aegis_program`,
  `attestation_schema` widened to `u64`, `attestation_mask` removed.
- Capability invalidation via `policy_epoch` bump + TTL; fail-closed
  (`EligibilityStale`) on a missing/stale capability. ExtraAccountMetaList swaps
  the aegis program/issuer/attestation trio for the single capability account.

### Migration
- Breaking: aegis + argus account layouts changed; ship 06+07+09 together
  (guard/clawback tests migrated to the commitment + capability flow). ZK
  predicates, accreditation (08), and enterprise governance (10) remain wave 2.

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
