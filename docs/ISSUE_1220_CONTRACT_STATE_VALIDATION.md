# Issue #1220: contract state validation

`contracts/split/src/contract_state_validation.rs` is a first slice of pure
helpers for validating invoice state transitions, checking invariants,
preventing invalid states, and logging state changes.

Implemented:

- `ContractState` — normalised invoice lifecycle states (Draft, Pending,
  Funded, PartiallyReleased, Released, Refunded, Expired, Cancelled, Disputed,
  Archived).
- `is_valid_transition` / `validate_transition` — explicit allowed edges; a
  transition must change state, `Archived` is terminal.
- `check_invariants` / `is_valid_state` — non-negative total, `funded` within
  `[0, total_due]`, recipients and deadline required off-draft, and
  funding-consistency per state (`Funded`/`Released` fully funded,
  `PartiallyReleased` strictly partial).
- `StateChangeLog` — bounded append-only log (`MAX_LOG_ENTRIES`), rejects when
  full.
- `apply_transition` — validates the edge, checks the prospective snapshot's
  invariants, records the change, and commits the state only once logging
  succeeds (atomic; a full log leaves the snapshot untouched).

Remaining gaps against the issue:

- Not wired to contract storage or entrypoints; the authoritative status in
  `types.rs` is projected in, not read live.
- No persisted log across transactions (in-memory only).
- Invariants cover amount/funding/recipient/deadline consistency, not the full
  invoice option surface.
- No integration with `contribute`/`release`/`refund` to enforce transitions on
  the real stored invoice.
