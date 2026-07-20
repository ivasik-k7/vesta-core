# Program metadata, security.txt & icons

The full set of on-chain discoverability + security resources for the three
VESTA programs. There are two independent layers; we ship both.

## 1. Embedded `security.txt` (in the ELF) — already in source

Each program embeds a `security.txt` via the `solana-security-txt` crate
(`security_txt!` in every `lib.rs`, gated `#[cfg(not(feature = "no-entrypoint"))]`
so it only lands in the deployed `.so`, never in the crate-as-dependency).
Solana Explorer parses it from the binary — nothing to run, it ships with the
next `anchor deploy`. Verify locally:

```bash
# after `cargo build-sbf`
strings target/deploy/argus.so | grep -A2 security.txt
```

## 2. On-chain program metadata (name, logo/icon, security) — deploy-time

Uses the official `solana-program/program-metadata` CLI. Writes a **canonical**
metadata account (PDA of `[program id, seed]`) that the program's **upgrade
authority** signs for. Explorers read these for the program name + icon.

Files in this directory, per program:

- `<program>/metadata.json` — `name`, `logo` (icon URL), `description`, `project_url`
- `<program>/security.json` — structured security contact (the `security` seed)
- `logos/<program>.svg` — the icon the `logo` URL should resolve to

Publish (run once per program, from the upgrade-authority wallet):

```bash
# name + icon (the "metadata" seed)
npx @solana-program/program-metadata@latest write metadata \
  9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx ./metadata/argus/metadata.json

# structured security contact (the "security" seed)
npx @solana-program/program-metadata@latest write security \
  9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx ./metadata/argus/security.json
```

Repeat with the aegis (`AcCdMQC1rj4KukjhFzf4S8metEAXpnt9gzvMThsu15e1`) and
vesta_core (`gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz`) ids and their JSON.

**Costs.** Each seed writes one account. On-chain JSON is zlib-compressed;
these payloads are ~200–400 bytes ⇒ roughly **0.002–0.004 SOL rent each**
(reclaimable by closing the account), plus signature fees. Six accounts
(metadata + security × 3 programs) ≈ **< 0.02 SOL** total. Using `--url` to
store only a pointer is even cheaper if you'd rather host the JSON off-chain.

The `logo` field is a URL — the SVGs here resolve at
`raw.githubusercontent.com/ivasik-k7/vesta-core/main/metadata/logos/…` once
pushed. Swap to a CDN/IPFS URL if you prefer immutability.

## 3. Token icon (the point token, not a program)

A VESTA point token's icon is **not** a program-metadata concern — it comes
from the Token-2022 `TokenMetadata` `uri` set in `register_merchant`. Point
that `uri` at a JSON like:

```json
{
  "name": "Kavarna Points",
  "symbol": "PTS",
  "description": "Loyalty points at Kavarna — decay unless you stay active.",
  "image": "https://.../points-icon.png"
}
```

Wallets and explorers render `image` as the token icon. The on-chain metadata
(name/symbol/uri) is already written by `register_merchant`; only the hosted
JSON + image need to exist at that `uri`.

## 4. Verified builds (recommended before mainnet)

`solana-verify` publishes a reproducible-build attestation so explorers show a
"verified" badge linking to this source. Out of scope on devnet; listed here so
the mainnet checklist is complete.
