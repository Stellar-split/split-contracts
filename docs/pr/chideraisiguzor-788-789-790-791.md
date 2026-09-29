# Reward pool info/events, error-code docs, capped co-funding rounds, split-view contract

Each issue gets its core piece plus tests. The remaining requirements are listed under "Not done in this PR", along with two constraints and two existing bugs found along the way.

## #788 - invoice reward pool

Already on `main`: `InvoiceOptions.bonus_pool` is transferred from the creator at creation and, on release, split between the first `bonus_max_payers` unique payers.

**Done**
- `get_reward_pool(invoice_id) -> RewardPoolInfo { pool_amount, top_n, distributed }`.
- `RewardDistributed` event (`split`, `rwd_dist`, invoice_id; data = (recipient, amount)) for each payout.
- Tests: not distributed before release (no events, `distributed = false`); on release the first N payers receive equal shares with one event each; fewer payers than N shares the pool among all of them.

**Not done in this PR**
- Ranking by contribution size ("top N"). Payers are still chosen by payment order, and `test_bonus_pool_distributed_to_first_payer` pins that behaviour, so changing it needs a maintainer decision.
- Max 10 for `top_n`, the `RewardPoolFunded` event, and renaming the options to `reward_pool` / `top_n_rewarded`.

Existing bug found (not changed here): when a `pay` call completes funding and auto-releases, that payment has been written to sharded storage but not to the in-memory `invoice.payments` the release uses. The final payer is therefore left out of the bonus distribution; if they are the only payer beyond the first, the first payer receives the whole pool. The tests hold release with `scheduled_release_at` and release separately to avoid it.

## #789 - diagnostic events on error paths

Soroban rolls back every contract event published by an invocation that fails, so a `DiagnosticError` event emitted just before a panic is never observable, on-chain or in tests. The issue's approach can't work as written.

**Done**
- `docs/ERROR_CODES.md`: all 66 `ContractError` codes with name and meaning (from the enum's doc comments), plus the rollback note above.
- `test_789_error_codes_doc_matches_enum`: parses `error.rs` and the doc table and fails if the (code, name) sets differ (checked by planting a wrong code).

**Not done in this PR**
- `DiagnosticError` events before each panic (see above). Structured failure context is the `Error(Contract, #code)` a caller receives.

## #790 - co-funding round with hard cap

**Done**
- `set_funding_round(creator, invoice_id, hard_cap, round_end)`: creator-only, while Pending, set once, `0 < hard_cap <= invoice total`, `round_end` in the future.
- `close_round(invoice_id)`: callable by anyone after `round_end`, once. If `funded > hard_cap`, the excess is refunded to payers in proportion to their contributions (the last payer absorbs rounding) and `funded` is reduced to the cap. Emits `RoundClosed` (`split`, `rnd_close`, invoice_id; data = (total_raised, overflow, refunded_count)).
- `get_round_info(invoice_id) -> RoundInfo { total_raised, hard_cap, round_end, closed, overflow }`: live while open, frozen at close.
- Tests: under cap -> no refund; 800 raised against a 600 cap -> 25% of each contribution refunded (75 / 75 / 50), `funded` 600; close before `round_end` panics; second close panics.

**Not done in this PR**
- Payments beyond the invoice total (the pay path still caps at the total, so `hard_cap` must be at most the total).
- Suppressing auto-release when funding reaches the invoice total before the round closes (`close_round` then panics "invoice is not pending").
- `hard_cap` / `round_end` in `InvoiceOptions` (at the 40-field `#[contracttype]` limit; setter used instead).

## #791 - read-only view contract

Soroban contracts can't read another contract's storage, so the view calls the split contract's public getters rather than sharing its key namespace.

**Done**
- New workspace crate `contracts/split-view` (`SplitViewContract`, builds to wasm):
  - `initialize(split_contract)` (once), `get_split_contract()`.
  - `get_creator_dashboard(creator) -> CreatorDashboard { total_invoices, total_raised, total_released, total_payers, total_refunded }` via `get_creator_stats`.
  - `get_payer_history(payer, limit) -> Vec<PaymentRecord>`: the latest `limit` entries via `get_payer_history`.
  - Mirror `CreatorStats` / `PaymentRecord` types (same XDR shape as `split::types`).
- Tests register the real split contract (dev-dependency) and query through the view: init once, dashboard equals `get_creator_stats`, payer history returns the latest entries in order.

**Not done in this PR**
- `get_global_leaderboard`, and `active_count` / `top_invoice_id` on the dashboard (the split contract has no getter or index for them).

Limitations inherited from the split contract (not changed here): payer history is only recorded by `contribute`, not `pay`, and creator `total_released` is only updated on tranche releases.

## Verification

- `cargo test --workspace`: all pass (`split` 344 -> 352, new `split-view` 3; other crates unchanged).
- `cargo clippy --all-targets`: no warnings on added lines in `split`, none in `split-view`.
- `cargo fmt --check`: no diffs in added code (pre-existing unformatted code on `main` left untouched).
- `cargo build -p split-view --target wasm32-unknown-unknown --release` succeeds.

Closes #788
Closes #789
Closes #790
Closes #791
