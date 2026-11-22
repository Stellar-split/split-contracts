# Contract security best practices

Practical guidance for integrating with and reviewing these Soroban contracts, written against
the patterns actually used in this repository (`contracts/split`, `contracts/invoice-escrow`,
`contracts/registry`, `contracts/factory`, and the rest of the workspace members listed in the
root `Cargo.toml`). This complements [`ERROR_CODES.md`](ERROR_CODES.md) (what each error means)
and [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md) (how to recover from it).

## 1. Authorization — check `require_auth` on every state-changing call

Every entrypoint that moves funds or changes privileged state authenticates the relevant party
with `Address::require_auth()` before mutating storage. Examples:

- `contracts/invoice-escrow/src/lib.rs:612` — `initialize` requires the new admin's auth.
- `contracts/invoice-escrow/src/lib.rs:753` — `create_invoice` requires the creator's auth.
- `contracts/invoice-escrow/src/lib.rs:820` — `deposit` requires the payer's auth before any
  token transfer.
- `contracts/invoice-escrow/src/lib.rs:147` (`require_admin` helper) — centralizes the
  "caller must be the stored admin" check so every admin-only entrypoint calls one function
  instead of re-implementing the check.

**When reviewing or extending a contract:** if you add a new entrypoint that reads a caller
address as a parameter (rather than deriving it from `env.invoker()` or similar), always call
`require_auth()` on it *before* any storage write or token transfer. A parameter that is never
authenticated is a classic "anyone can act as anyone" bug — the caller supplies whichever
`Address` they want and nothing stops them from impersonating another account.

The arbitration entrypoints added in this change follow the same shape: `set_arbiter` reuses
`require_admin`, `raise_dispute` requires the caller's own auth plus a creator/co-signer
membership check, and `arbitrate_dispute` requires the *configured* arbiter's auth — comparing
the passed-in `arbiter` against stored state before calling `require_auth`, so a caller cannot
claim to be the arbiter by simply passing a different address.

## 2. Validate before you mutate

`contracts/invoice-escrow/src/lib.rs` deposit/release/refund functions are structured as
"--- Validate all preconditions before any mutation ---" followed by "--- Mutations begin
here ---" (see `deposit` at line ~835, `release` at line ~905, `refund` at line ~958). This
ordering matters: if a precondition check happens *after* a token transfer or storage write,
a failing transaction can leave the contract and an external token balance out of sync, because
Soroban rolls back storage on a trapping call but a cross-contract token transfer that already
executed cannot always be assumed reversible in every host/token implementation. Keep all
`Err(...)` returns ahead of the first `token_client.transfer(...)` or `storage().set(...)` call.

## 3. Arithmetic — use `checked_*`, never bare `+`/`-`/`*` on amounts

This codebase uses `checked_add` / `checked_sub` / `checked_mul` widely (122 call sites across
the workspace at last count) for anything that touches a balance, counter, or threshold, e.g.:

- `contracts/invoice-escrow/src/lib.rs:760` — the invoice counter uses
  `id.checked_add(1).expect("invoice counter overflow")`.
- `contracts/invoice-escrow/src/lib.rs:844` — `funded_amount.checked_add(amount).expect(...)`.

`overflow-checks = true` is also set in the workspace `Cargo.toml` release profile, which turns
silent wraparound into a trap in release builds — but that is a safety net, not a substitute for
explicit `checked_*` arithmetic. A trap mid-transaction still aborts the whole call (losing the
caller's submitted fee) and gives no actionable error code, whereas a `checked_*` call lets you
return a specific `Error` variant (see `Error::InvalidAmount`, `Error::OverFunded`) with a clear
recovery path. When adding new balance or counter math, prefer `checked_add(...).ok_or(Error::X)`
over relying on the overflow-checks profile flag alone.

## 4. `.unwrap()` / `.expect()` on untrusted input is a liveness risk

`contracts/split/src/lib.rs` has many `.get(i).unwrap()` calls (e.g. lines ~467, ~3729, ~5295,
~5452) when iterating internal `Vec`s whose indices are bounds-checked by the surrounding loop —
that pattern is fine because the index is derived from `.len()`, not from caller input. The risk
is the opposite case: an `.unwrap()` on a value derived *directly* from a caller-supplied index,
address, or ID. Before adding a new `.unwrap()`, ask: "can a caller control the value that makes
this `None`/`Err`?" If yes, return a typed `Error` instead (see `errors.rs` in each contract for
the existing variant style) so a malicious or malformed call degrades to a clean error rather than
a panic that burns the caller's fee with no diagnostic code.

## 5. Two-step transfers for privileged roles

Admin handover in `invoice-escrow` is two-step (`transfer_admin` → `accept_admin`, with
`cancel_transfer` to abort), not a single `set_admin(new_admin)` call — see
`contracts/invoice-escrow/src/lib.rs:622-695`. This prevents an irrecoverable lock-out from a
typo'd address, since the new admin must actively authenticate to accept the role before the
handover completes. Apply the same two-step pattern to any other privileged singleton address
you add (we did for `set_arbiter` deliberately keeping it single-step and admin-gated instead,
since the arbiter is replaceable by the admin at any time and is not itself privileged to drain
funds — only to pick between two outcomes on an already-disputed invoice).

## 6. Reentrancy: cross-contract calls happen after local state is already consistent

Soroban's execution model does not have the same reentrancy surface as EVM (no fallback
functions, and token transfers are synchronous host calls), but this codebase still follows the
checks-effects-interactions pattern defensively: `execute_release`
(`contracts/invoice-escrow/src/lib.rs:380`) flips `invoice.status = EscrowStatus::Released` and
calls `save_invoice` *before* calling `token_client.transfer(...)`. If a token contract were ever
malicious or buggy and tried to call back into `invoice-escrow` during that transfer, it would
see an already-`Released` invoice and get `Error::InvalidStatus` rather than being able to drain
funds twice. Keep this ordering (state mutation, then external call) for any new entrypoint that
both changes status and moves tokens — including the `ForceRelease` / `ForceRefund` branches in
`arbitrate_dispute`, which mark the dispute `Resolved` only after the underlying release/refund
logic (which itself follows the same state-then-transfer order) completes.

## 7. Blacklist and dispute windows are time-bounded by ledger state, not wall-clock trust

`APPEAL_WINDOW_LEDGERS` (`contracts/invoice-escrow/src/lib.rs:120`) gates the blacklist appeal
window by ledger sequence, not `env.ledger().timestamp()`, specifically because ledger sequence
cannot be manipulated by a validator the way reported close time theoretically could be over a
long window. The co-signer dispute path added alongside this guide intentionally has no such
window — a dispute stays `Open` until the arbiter decides — because unlike the blacklist appeal
(which must give a payer a bounded chance to respond before a one-sided ban is finalized), a
rejected release has no default "do nothing" outcome: funds are stuck until *someone* decides.
If you add a new time-boxed security control, default to ledger-sequence bounds over timestamp
bounds when the window needs to resist validator timestamp drift.

## Incident response checklist

If a security issue is found in a deployed contract:

1. **Freeze new harmful state, don't try to patch live Wasm.** Soroban contracts are upgraded by
   deploying new Wasm and migrating, not by hot-patching; use `cancel_invoice` /
   `transfer_admin` / `set_arbiter`-style admin controls already present in the contract to limit
   further damage (e.g. an admin can stop proposing new invoices via downstream tooling) while a
   fix is prepared.
2. **Identify the error/state path**, using [`ERROR_CODES.md`](ERROR_CODES.md) to map any
   abnormal `Error` codes seen in failed transactions back to the exact `errors.rs` variant and
   the code path in `lib.rs` that returned it.
3. **Check events, not just final state.** Every state change emits a topic'd event (see the
   `emit_*` helpers in each contract's `lib.rs`); reconstruct what happened from the event log
   before assuming storage reflects the full history, since storage only holds the latest
   invoice/approval/dispute record.
4. **Reproduce with a test**, mirroring the existing style in `contracts/invoice-escrow/src/test.rs`,
   before shipping a fix — this repo keeps tests alongside the contract they cover specifically so
   a regression test can be added in the same PR as the fix.
5. **Rotate privileged addresses** (admin, arbiter, co-signers) via the existing two-step/admin-
   gated entrypoints if a key may have been compromised, rather than assuming compromise requires
   a new contract deployment.

## References

- `contracts/invoice-escrow/src/errors.rs` — authoritative list of error variants and discriminants.
- `contracts/invoice-escrow/src/lib.rs` — entrypoint implementations referenced above.
- Workspace `Cargo.toml` — `overflow-checks = true` release profile setting.
- [Soroban security docs](https://developers.stellar.org/docs/build/guides/security) — platform-level
  guidance on authorization, storage, and token interactions that this guide builds on.
