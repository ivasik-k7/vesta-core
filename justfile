# VESTA on-chain workspace tasks. `just` with no args runs the full local gate.

set shell := ["bash", "-cu"]

export PATH := env_var("HOME") + "/.cargo/bin:" + env_var("HOME") + "/.local/share/solana/install/active_release/bin:" + env_var("PATH")

default: check

# full local gate — what CI runs
check: fmt-check lint build test deny machete

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

lint:
    cargo clippy --all-targets -- -D warnings

fix:
    cargo clippy --all-targets --fix --allow-dirty --allow-staged
    cargo fmt --all

# build both programs to target/deploy/*.so (also feeds the LiteSVM tests)
build:
    cargo build-sbf

test:
    cargo nextest run

# supply chain: advisories, licenses, bans, sources
deny:
    cargo deny check

machete:
    cargo machete

typos:
    typos

deploy-devnet:
    anchor deploy
    anchor idl upgrade --filepath target/idl/vesta_core.json Am2X4B1SCnJKXL8Yir2j6yGpHAKrmwcf2E5aKnA9BZV || anchor idl init --filepath target/idl/vesta_core.json Am2X4B1SCnJKXL8Yir2j6yGpHAKrmwcf2E5aKnA9BZV

doc:
    cargo doc --no-deps --workspace
