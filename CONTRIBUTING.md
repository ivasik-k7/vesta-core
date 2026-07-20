# Contributing to VESTA

Thank you for your interest in the project. VESTA is on-chain financial-adjacent
infrastructure, so contributions are held to a strict bar: **security first,
correctness second, everything else third.**

## Ground rules

- All discussion, code, comments, and commit messages are in **English**.
- Anything exploitable goes to **kovtun.ivan@proton.me** per [SECURITY.md](SECURITY.md),
  never to the public issue tracker.
- By submitting a contribution you agree it is licensed under the repository's
  [MIT license](LICENSE) and that you have the right to submit it.

## Development setup

| Tool | Version |
|---|---|
| Rust | 1.89 (pinned in `rust-toolchain.toml`) |
| Solana CLI (Agave) | 4.1.x |
| Anchor | 1.1.2 |
| Node.js | ≥ 20 |

```bash
npm install
anchor build          # builds vesta_core, argus, aegis
cargo test            # LiteSVM integration suite
pre-commit install    # fmt / clippy / prettier hooks
```

## Quality gates

Every change must pass locally what CI enforces:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check      # licenses & advisories (deny.toml)
```

Non-negotiables for on-chain code:

- **No unchecked arithmetic.** `checked_*` / `saturating_*` only —
  `clippy::arithmetic_side_effects` is `deny` at the workspace level.
- **No `unsafe`.** Forbidden at the workspace level.
- **Fail closed.** Guard paths must reject on any inconsistency; never add a
  permissive fallback.
- **Pinned derivations.** Cross-program account resolution derives PDAs from
  pinned program IDs and seeds; never trust client-supplied accounts for policy.
- **Two-step authority transfers** for any new authority-bearing account.
- Any change to an account layout requires a migration note and updated size
  constants; `Merchant.id` must stay at a fixed offset (argus reads raw bytes).

## Tests

New instructions or invariants ship with LiteSVM coverage: the happy path plus
at least the authority-violation and cap/limit rejection paths. A bug fix ships
with a regression test that fails before the fix.

## Commits & pull requests

- Conventional commits (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`).
- One logical change per PR; describe *what* and *why*, link the issue,
  list the tests that prove it.
- PRs touching guard logic, authority checks, or arithmetic must call that out
  explicitly in the description so review effort lands where the risk is.
- CI must be green; the maintainer reviews all changes (see `.github/CODEOWNERS`).

## Releases

Devnet deploys are performed by the maintainer from `main`. Program upgrades
follow the [operations runbook](README.md#operations-runbook); mainnet is gated
on external audit and multisig custody.
