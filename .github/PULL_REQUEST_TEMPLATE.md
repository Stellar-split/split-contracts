## What changed

<!-- What this PR does and why, in a few lines. -->

Closes #

## How it was tested

<!-- The commands you ran and what you checked. -->

## Checklist

- [ ] The issue was assigned to me before I started
- [ ] The branch is named `fix/issue-N-short-description` or `feat/issue-N-short-description`
- [ ] The PR title is under 70 characters and the description has `Closes #N`
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy` shows no new warnings and `cargo fmt` was run
- [ ] New public functions have `///` doc comments
- [ ] No `unwrap()` in production code paths
- [ ] A new `ContractError` variant is appended at the end with the next free number
- [ ] A change to a stored `#[contracttype]` struct bumps `CURRENT_SCHEMA_VERSION` and adds a migration
- [ ] A new storage key is added to `STORAGE_KEY_REGISTRY.md`, and the storage key snapshot was updated if it changed
- [ ] User-visible behaviour is described in `README.md` or `docs/`
