## Summary

<!-- What does this change and why? Link the issue if one exists. -->

## Risk surface

- [ ] Touches guard / transfer-hook logic (argus)
- [ ] Touches authority checks or PDA derivations
- [ ] Touches arithmetic (all ops `checked_*` / `saturating_*`)
- [ ] Changes an account layout (migration note + size constants updated)
- [ ] None of the above

## Verification

<!-- Tests added/updated; commands run and their results. -->

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] `cargo deny check`
