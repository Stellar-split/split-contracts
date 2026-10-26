# StellarSplit Comprehensive Troubleshooting Guide

> **Issue**: `#1035`  
> **Scope**: `contracts/split`, `contracts/invoice-escrow`, local development/CI, and Stellar Testnet/Mainnet operations  
> **Companion Docs**: [`docs/ERROR_CODES.md`](./ERROR_CODES.md) · [`docs/API.md`](./API.md) · [`STORAGE_KEY_REGISTRY.md`](../STORAGE_KEY_REGISTRY.md)

---

## Table of Contents

1. [Quick Diagnostic Triage Flowchart](#1-quick-diagnostic-triage-flowchart)
2. [Diagnostic Tools & On-Chain Inspection Commands](#2-diagnostic-tools--on-chain-inspection-commands)
3. [Step-by-Step Troubleshooting by Category](#3-step-by-step-troubleshooting-by-category)
   - [3.1 Build, Toolchain & Snapshot Test Issues](#31-build-toolchain--snapshot-test-issues)
   - [3.2 Invoice Creation & Validation Errors](#32-invoice-creation--validation-errors)
   - [3.3 Payment, FX Oracle & Meta-Transaction Failures](#33-payment-fx-oracle--meta-transaction-failures)
   - [3.4 Release, Refund, Milestone & Dispute Locks](#34-release-refund-milestone--dispute-locks)
   - [3.5 Rate Limiting, Emergency Pause & Circuit Breaker Blocks](#35-rate-limiting-emergency-pause--circuit-breaker-blocks)
   - [3.6 Contract Upgrades, Rollbacks & Storage TTL Expiry](#36-contract-upgrades-rollbacks--storage-ttl-expiry)
4. [Complete `contracts/split` Error Code Reference (`#1`–`#77`)](#4-complete-contractssplit-error-code-reference-177)
5. [Complete `contracts/invoice-escrow` Error Code Quick Reference](#5-complete-contractsinvoice-escrow-error-code-quick-reference)
6. [Frequently Asked Questions (FAQ)](#6-frequently-asked-questions-faq)

---

## 1. Quick Diagnostic Triage Flowchart

When a contract call fails with `HostError: Error(Contract, #N)` or an unexpected state transition occurs, follow this decision flow:

```
Transaction Failed / Unexpected Contract Behavior
 │
 ├─► Did the transaction fail with `Error(Contract, #N)`?
 │     ├─► YES: Query on-chain error metadata & recovery suggestion:
 │     │        `stellar contract invoke ... -- get_error_description --code N`
 │     │        `stellar contract invoke ... -- get_recovery_suggestion --code N`
 │     │        Look up `#N` in Section 4 (`contracts/split`) or Section 5 (`invoice-escrow`).
 │     │
 │     └─► NO (HostError: Auth / Budget / Storage / WasmVm):
 │           ├─► `Error(Auth, ...)`: Missing `require_auth` signature or expired auth entry.
 │           ├─► `Error(Budget, ExceededLimit)`: Batch size or recipient count too large; split into smaller calls.
 │           └─► `Error(Storage, MissingValue)`: Uninitialized contract or expired ledger entry TTL (`extend_ttl`).
 │
 └─► Is an invoice stuck in `Pending`, `Funded`, or `Disputed`?
       ├─► Run dry-run release check: `is_invoice_releasable(invoice_id)` & `simulate_release(invoice_id)`
       ├─► Run invoice health audit:  `run_invoice_audit(invoice_id)` & `verify_invoice_invariant(invoice_id)`
       └─► Check global/function pause: `get_pause_state()` & `is_function_paused(func_name)`
```

---

## 2. Diagnostic Tools & On-Chain Inspection Commands

StellarSplit embeds dedicated read-only diagnostic and dry-run functions directly inside `contracts/split` so operators and integrators can diagnose failures without spending gas on mutating calls.

### 2.1 On-Chain Error Decoder & Recovery Helper

Query the human-readable description and actionable recovery suggestion for any numeric `ContractError` code directly from the contract:

```bash
# Decode error code #7 (InvoiceNotFunded)
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- get_error_description --code 7

# Get actionable recovery steps for error code #7
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- get_recovery_suggestion --code 7
```

### 2.2 Dry-Run Release & Invoice State Inspector

Before submitting a `release` transaction, inspect the invoice state and simulate the release calculation:

```bash
# 1. Inspect full invoice state (status, total_amount, total_paid, deadline, disputed, etc.)
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- get_invoice --invoice_id 42

# 2. Check whether the invoice satisfies all release preconditions
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- is_invoice_releasable --invoice_id 42

# 3. Simulate exact recipient payouts and protocol fees without mutating state
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- simulate_release --invoice_id 42
```

### 2.3 Security, Invariant & Monitoring Diagnostics

```bash
# Run comprehensive contract health & configuration audit (returns SecurityAuditReport)
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- run_contract_audit

# Audit a specific invoice for accounting discrepancies or stuck locks
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- run_invoice_audit --invoice_id 42

# Verify strict conservation of value invariant on an invoice
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- verify_invoice_invariant --invoice_id 42

# Inspect live operational metrics, error rates, and active alerts
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- get_monitoring_dashboard
```

### 2.4 Pause, Circuit Breaker & Rate Limit Diagnostics

```bash
# Check global emergency pause state and circuit breaker status
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- get_pause_state

# Check rate limit quota and penalty status for a specific user address
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ACCOUNT> \
  --network testnet \
  -- get_rate_limit_status --user <USER_ADDRESS>
```

---

## 3. Step-by-Step Troubleshooting by Category

### 3.1 Build, Toolchain & Snapshot Test Issues

#### Issue A: `package soroban-sdk@28.0.0 cannot be built because it requires rustc 1.91.0 or newer`
- **Cause**: Your default `rustc` toolchain is older than `1.91.0` (e.g., `1.88.0`).
- **Solution**:
  1. Install and select a Rust toolchain `>= 1.91.0` (e.g., `1.94.1`):
     ```bash
     rustup toolchain install 1.94.1
     rustup target add wasm32-unknown-unknown --toolchain 1.94.1
     RUSTUP_TOOLCHAIN=1.94.1 cargo test --workspace
     ```

#### Issue B: `storage_snapshot` Test Fails After Adding or Modifying a Storage Key
- **Symptom**: `cargo test -p split storage_snapshot` panics with `Storage key snapshot mismatch!`.
- **Cause**: StellarSplit enforces a strict XDR snapshot baseline in `tests/snapshots/storage_keys.json` (`STORAGE_KEY_REGISTRY.md`) to prevent accidental storage key mutations and XDR collisions.
- **Solution**:
  1. Ensure your new key helper is defined in `contracts/split/src/storage_keys.rs` and registered in `all_key_entries()` inside `contracts/split/src/storage_snapshot.rs`.
  2. Verify no two keys produce identical XDR bytes (`duplicate XDR found` assertion).
  3. Regenerate the snapshot baseline:
     ```bash
     RUSTUP_TOOLCHAIN=1.94.1 cargo test -p split storage_snapshot 2>&1 \
       | grep -A 999 "EXPECTED (generated)" \
       | tail -n +2 | head -n -1 > tests/snapshots/storage_keys.json
     ```
  4. Re-run `RUSTUP_TOOLCHAIN=1.94.1 cargo test -p split storage_snapshot` to confirm it passes.

---

### 3.2 Invoice Creation & Validation Errors

#### Issue A: `create_invoice` Fails with `#3` (`InvalidSplit`) or `#1` (`RecipientsAmountsMismatch`)
- **Cause**:
  - `recipients.len() != amounts.len()`, or `recipients` is empty (`#2 EmptyRecipients`).
  - For percentage/bps split invoices, the shares do not sum to `10_000` bps (`100%`).
  - A recipient amount is `<= 0` (`#10 InvalidAmount`).
- **Solution**:
  1. Ensure `recipients` and `amounts` vectors have identical, non-zero lengths (`1..=MAX_RECIPIENTS`).
  2. Verify every amount is strictly positive (`> 0`).
  3. For basis-point splits, verify `shares.iter().sum::<u32>() == 10_000`.

#### Issue B: `create_invoice` Fails with `#4` (`DeadlineInPast`)
- **Cause**: The supplied `deadline` Unix timestamp is `<= env.ledger().timestamp()`.
- **Solution**: Fetch the latest ledger close timestamp from your RPC node and set `deadline` to a future Unix timestamp (in seconds, not milliseconds).

#### Issue C: `create_invoice` Fails with `#26` (`DescriptionTooLong`) or `#38` (`TooManyMetadataEntries`)
- **Cause**: Input strings or metadata maps exceed configured `ValidationRules` limits.
- **Solution**: Query `get_validation_rules(env)` and truncate descriptions or reduce metadata entries to fit within `max_string_len` and `max_metadata_entries`.

---

### 3.3 Payment, FX Oracle & Meta-Transaction Failures

#### Issue A: `pay` Fails in SAC Token Transfer (`HostError: Error(Contract, #...)` in Token Contract)
- **Cause**:
  1. The `payer` has insufficient balance of the invoice's `token` (e.g., USDC).
  2. The `payer` has not signed the transaction (`payer.require_auth()` failed).
  3. The `token` address on the invoice is not a valid deployed Stellar Asset Contract.
- **Solution**:
  1. Check payer token balance: `stellar contract invoke --id <TOKEN_ID> -- balance --id <PAYER>`.
  2. Ensure the transaction is signed by `<PAYER>` and includes the SAC transfer authorization entry.

#### Issue B: Multi-Currency Payment Fails with `#21` (`OracleRateStale`), `#19` (`InvalidOracleRate`), or `#20` (`SlippageExceeded`)
- **Cause**:
  - `#19`: No FX rate is registered for `(base_token, quote_token)`, or the rate is `<= 0`.
  - `#21`: `env.ledger().timestamp() - oracle_rate.updated_at` exceeds the configured staleness window.
  - `#20`: The converted amount deviates from the payer's expected amount by more than `max_slippage_bps`.
- **Solution**:
  1. Refresh the oracle price feed via `set_oracle_rate(env, oracle, base_token, quote_token, rate)` before submitting the payment.
  2. Or pay directly in the invoice's native `token` using `pay(env, payer, invoice_id, amount)`.

#### Issue C: Meta-Transaction (`pay_with_intent`) Fails with `#42` (`IntentExpired`) or `#43` (`InvalidSignature`)
- **Cause**:
  - `#42`: The signed `PaymentIntent.deadline` has already passed.
  - `#43`: The Ed25519 signature does not match the canonical serialized payload `(invoice_id, amount, nonce, deadline)` or the payer's current nonce (`get_nonce(payer)`) has already been consumed.
- **Solution**:
  1. Query the current nonce for the payer (`get_nonce`).
  2. Re-sign the `PaymentIntent` with a fresh future `deadline` and the exact current nonce.

---

### 3.4 Release, Refund, Milestone & Dispute Locks

#### Issue A: `release` Fails with `#7` (`InvoiceNotFunded`), `#11` (`ConditionsNotMet`), or `#12` (`InvoiceDisputed`)
- **Diagnosis Steps**:
  1. Inspect `get_invoice(env, invoice_id)`.
  2. If `status != Funded` (`#7`): Check `total_paid` vs. `total_amount`. Complete the remaining contribution first.
  3. If `disputed == true` (`#12`): The invoice is frozen by an active dispute. Wait for arbitrators to reach quorum and call `resolve_dispute`, or check `get_dispute(env, invoice_id)`.
  4. If `#11` (`ConditionsNotMet`): Inspect release conditions (`get_release_conditions`) or milestones (`get_milestones`). Ensure all required oracle/arbiter/milestone approvals are marked complete before releasing.

#### Issue B: `refund` Fails with `#9` (`DeadlineNotPassed`) or `#8` (`InvoiceAlreadySettled`)
- **Cause**:
  - Standard full-invoice `refund(env, invoice_id)` is only callable *after* `env.ledger().timestamp() > invoice.deadline` on an unfunded invoice.
  - If the invoice was already `Released`, `Refunded`, or `Cancelled`, `#8` (`InvoiceAlreadySettled`) is returned.
- **Solution**:
  - If the deadline has not passed and the creator wishes to abort before payments are locked, use `cancel_invoice` (if unfunded) or `partial_refund` (creator-authorized).
  - If the invoice is disputed and arbitrators vote to reject (`approve = false`), `resolve_dispute` will automatically refund payers regardless of deadline.

---

### 3.5 Rate Limiting, Emergency Pause & Circuit Breaker Blocks

#### Issue A: Calls Fail with `#23` (`ContractPaused`) orFunction-Specific Pause
- **Cause**: The contract is in global emergency pause or the specific entrypoint is paused via `EmergencyPauseManager`.
- **Solution**:
  1. Run `get_pause_state(env)` to see the `pause_reason`, `paused_at`, `auto_unpause_at`, and `paused_functions`.
  2. If the pause was triggered by an automated circuit breaker (`CircuitBreakerTripped`), admins must inspect `get_active_alerts(env)`, clear the underlying anomaly, and collect required multi-admin approvals via `approve_unpause` / `unpause`.

#### Issue B: Calls Fail with `#70` (`RateLimitExceeded`)
- **Cause**: The caller exceeded the per-window action limit (`max_actions_per_window`) or triggered a violation cooldown multiplier (`rate_limiting.rs`).
- **Solution**:
  1. Query `get_rate_limit_status(env, user)` to check `remaining_actions` and `window_reset_at`.
  2. Wait until `env.ledger().timestamp() >= window_reset_at`, or use batch endpoints (`batch_create_invoices`, `batch_pay`, `batch_release`) to process multiple items in a single rate-limit slot.

---

### 3.6 Contract Upgrades, Rollbacks & Storage TTL Expiry

#### Issue A: `execute_upgrade` Fails with `#72` (`UpgradeNotApproved`), `#74` (`InvalidUpgradeVersion`), or `#75` (`MigrationFailed`)
- **Cause**:
  - `#72`: The proposal has not yet met the multi-admin approval threshold (`required_approvals`).
  - `#74`: The proposed `ContractVersion` (`major.minor.patch`) is not strictly greater than the current active version.
  - `#75`: State migration validation failed.
- **Solution**:
  1. Run `simulate_upgrade(env, proposal_id)` first to verify version monotonicity, approval count, and migration readiness.
  2. Collect remaining admin approvals with `approve_upgrade(env, approver, proposal_id)`.
  3. If an executed upgrade behaves unexpectedly, invoke `rollback_upgrade(env, admin)` before the rollback window expires (`#76 RollbackWindowExpired`).

---

## 4. Complete `contracts/split` Error Code Reference (`#1`–`#77`)

All error codes defined in [`contracts/split/src/error.rs`](../contracts/split/src/error.rs) and queryable on-chain via `get_error_description(code)` / `get_recovery_suggestion(code)`:

| Code | Variant | Category | Description & Root Cause | Actionable Recovery Suggestion |
|------|---------|----------|--------------------------|--------------------------------|
| `1` | `RecipientsAmountsMismatch` | Validation | `recipients` and `amounts` vectors have different lengths | Ensure `recipients` and `amounts` lists have the exact same length |
| `2` | `EmptyRecipients` | Validation | `recipients` vector is empty | Provide at least one recipient address |
| `3` | `InvalidSplit` | Validation | Split shares do not sum to 10,000 bps (100%) or contain invalid entries | Verify all split shares are positive and sum to 10,000 bps |
| `4` | `DeadlineInPast` | Validation | Invoice `deadline` is `<=` current ledger timestamp | Set a deadline timestamp in the future (`> env.ledger().timestamp()`) |
| `5` | `InvoiceNotFound` | State | Requested `invoice_id` does not exist in storage | Check the `invoice_id` via `get_invoice` |
| `6` | `InvoiceAlreadyFunded` | State | Attempted to pay an invoice that is already fully funded | No further payment is needed; call `release` if conditions are met |
| `7` | `InvoiceNotFunded` | State | Attempted to `release` an invoice before full funding | Pay the remaining balance (`total_amount - total_paid`) before calling `release` |
| `8` | `InvoiceAlreadySettled` | State | Invoice is already `Released`, `Refunded`, or `Cancelled` | Invoice lifecycle is complete; create a new invoice if needed |
| `9` | `DeadlineNotPassed` | State | Attempted to `refund` before the invoice deadline expired | Wait until after the invoice deadline (`invoice.deadline`) to call `refund` |
| `10` | `InvalidAmount` | Validation | Amount is `<= 0` or fails validation bounds | Supply a strictly positive token amount (`> 0`) |
| `11` | `ConditionsNotMet` | Escrow | Release conditions or milestones are not yet satisfied | Satisfy all release conditions / milestones before calling `release` |
| `12` | `InvoiceDisputed` | Dispute | Invoice is currently frozen under an active dispute | Resolve the active dispute via `resolve_dispute` before attempting release or refund |
| `13` | `Unauthorized` | Auth | Caller is not authorized for this action | Ensure the transaction is signed by an authorized address with required permissions |
| `14` | `MilestoneNotFound` | Milestone | Specified `milestone_id` does not exist on the invoice | Check valid milestone indices with `get_milestones` |
| `15` | `MilestoneAlreadyReleased` | Milestone | Milestone funds were already released | Milestone has already been paid out |
| `16` | `ScheduleNotDue` | Recurring | Recurring invoice interval has not yet elapsed | Wait until `next_due_date` before triggering the next recurring invoice |
| `17` | `ScheduleInactive` | Recurring | Recurring schedule is paused, completed, or cancelled | Create a new recurring schedule |
| `18` | `DisputeAlreadyOpen` | Dispute | A dispute is already open for this invoice | Wait for the existing dispute to be resolved |
| `19` | `InvalidOracleRate` | FX / Oracle | Oracle rate is missing or `<= 0` | Configure a valid positive oracle rate via `set_oracle_rate` |
| `20` | `SlippageExceeded` | FX / DEX | FX conversion or swap output fell below slippage tolerance | Increase `max_slippage_bps` or update the oracle rate |
| `21` | `OracleRateStale` | FX / Oracle | Oracle rate timestamp is older than maximum staleness | Push a fresh rate update from the authorized oracle address |
| `22` | `FeeTooHigh` | Config | Protocol fee exceeds `MAX_FEE_BPS` (`1000` = 10%) | Set `fee_bps <= 1000` |
| `23` | `ContractPaused` | Emergency | Contract is paused globally or for this function | Wait for an admin to unpause the contract after incident resolution |
| `24` | `TooManyRecipients` | Validation | Recipient count exceeds `MAX_RECIPIENTS` | Reduce recipients per invoice or split across multiple invoices |
| `25` | `DeadlinePassed` | State | Action is not allowed after the invoice deadline has passed | The invoice deadline has expired; call `refund` if partially funded |
| `26` | `DescriptionTooLong` | Validation | Description string exceeds maximum byte length | Shorten the description string to fit `ValidationRules::max_string_len` |
| `27` | `NotDisputed` | Dispute | Attempted to vote on or resolve an invoice with no open dispute | Open a dispute first via `open_dispute` |
| `28` | `InvoiceNotPending` | State | Action requires invoice to be in `Pending` status | Action is only valid while the invoice is in `Pending` status |
| `29` | `Overflow` | Arithmetic | Checked integer arithmetic overflowed or underflowed | Reduce input amounts to prevent `i128`/`u64` overflow |
| `30` | `NoClaimableBalance` | Payout | Caller has `0` claimable balance in pull-payment escrow | Verify recipient address and whether the invoice has been released |
| `31` | `InvalidCommitment` | Privacy | Pedersen commitment failed verification (`sum(C_i) != C_total`) | Recompute commitments with matching blinding factors |
| `32` | `CommitmentMismatch` | Privacy | Revealed amount + blinding factor does not match stored commitment | Supply the exact amount and blinding factor used during commitment creation |
| `33` | `ScheduleNotFound` | Recurring | Recurring `schedule_id` does not exist | Verify `schedule_id` |
| `34` | `BatchTooLarge` | Batch | Batch size exceeds `MAX_BATCH_SIZE` or is empty | Split the batch into smaller chunks (`1..=MAX_BATCH_SIZE`) |
| `35` | `NotPartiallyFunded` | State | Partial release/refund requires partial funding | Ensure at least one payment has been made |
| `36` | `NothingToRelease` | Payout | Unreleased funded amount is `0` | All currently funded shares have already been released |
| `37` | `InvalidMetadataKey` | Metadata | Metadata key is empty or exceeds max key length | Use a non-empty alphanumeric key within length bounds |
| `38` | `TooManyMetadataEntries` | Metadata | Metadata map exceeds `max_metadata_entries` | Remove unused metadata keys before adding new ones |
| `39` | `ApprovalThresholdNotMet` | Multi-Sig | Multi-sig operation lacks required signers | Collect additional approvals via `approve_operation` |
| `40` | `DuplicateSigner` | Multi-Sig | Signer has already approved or is duplicated in signer set | Use distinct signer addresses |
| `41` | `InvalidThreshold` | Multi-Sig | Quorum threshold is `0` or `> signers.len()` | Set threshold between `1` and `signers.len()` |
| `42` | `IntentExpired` | Meta-Tx | Signed `PaymentIntent` deadline has passed | Generate and sign a new `PaymentIntent` with a future deadline |
| `43` | `InvalidSignature` | Crypto | Ed25519 signature verification failed | Verify public key, message serialization, and signature bytes |
| `44` | `AlreadyInitialized` | Init | Contract has already been initialized | Contract initialization is one-time only |
| `45` | `NotInitialized` | Init | Contract has not been initialized yet | Call `initialize` before invoking business entrypoints |
| `46` | `YieldActive` | Yield | Cannot modify or settle while funds are deployed in yield protocol | Withdraw funds from the yield vault first (`withdraw_from_yield`) |
| `47` | `YieldNotActive` | Yield | Attempted yield withdrawal when no yield position is active | Deposit funds into yield first (`deposit_to_yield`) |
| `48` | `ReentrancyDetected` | Security | Concurrent reentrant invocation blocked by `ReentrancyGuard` | Complete the outer call before invoking another state-mutating entrypoint |
| `49` | `InvalidCancelWindow` | Cancellation | Cancellation window configuration is invalid | Set a valid cancellation window duration |
| `50` | `CancelWindowExpired` | Cancellation | Grace period for cancellation has expired | Use dispute resolution or wait for deadline expiry |
| `51` | `PenaltyTooHigh` | Config | Cancellation or late penalty exceeds maximum bps | Set penalty bps within allowed limits |
| `52` | `ReceiptNotFound` | Receipt | Payment receipt ID does not exist | Verify receipt ID via `get_user_receipts` |
| `53` | `TemplateNotFound` | Template | Invoice template ID does not exist | Verify template ID via `get_template` |
| `54` | `DependencyNotMet` | Workflow | Prerequisite invoice dependency is not yet settled | Settle the parent dependency invoice first |
| `55` | `CircularDependency` | Workflow | Invoice dependency graph contains a cycle | Remove the cyclic dependency reference |
| `56` | `AuctionNotFound` | Auction | Invoice factoring auction ID does not exist | Check `auction_id` |
| `57` | `AuctionEnded` | Auction | Bid placed after auction end time | Participate in an active auction before `end_time` |
| `58` | `AuctionNotEnded` | Auction | Attempted to settle auction before `end_time` | Wait until `env.ledger().timestamp() >= auction.end_time` |
| `59` | `BidTooLow` | Auction | Bid is below `min_bid` or current highest bid | Submit a higher bid amount |
| `60` | `NotificationNotFound` | Notification | Notification ID does not exist | Verify notification ID |
| `61` | `PoolNotFound` | Insurance | Insurance pool does not exist | Initialize the insurance pool first |
| `62` | `ClaimNotFound` | Insurance | Insurance claim ID does not exist | Verify `claim_id` |
| `63` | `ClaimAlreadyProcessed` | Insurance | Insurance claim was already approved or rejected | No further action needed |
| `64` | `InsufficientPoolFunds` | Insurance | Insurance pool balance is lower than claim payout | Fund the insurance pool before processing the claim |
| `65` | `StreamNotFound` | Streaming | Payment stream ID does not exist | Verify `stream_id` |
| `66` | `StreamNotActive` | Streaming | Payment stream is paused, cancelled, or completed | Check stream status via `get_stream` |
| `67` | `NothingToWithdraw` | Streaming | Streamed accrued balance available for withdrawal is `0` | Wait for additional time to elapse on the active stream |
| `68` | `OrderNotFound` | OrderBook | Split order ID does not exist | Verify `order_id` |
| `69` | `OrderNotOpen` | OrderBook | Order is already filled or cancelled | Select an open order |
| `70` | `RateLimitExceeded` | Security | Caller exceeded sliding-window rate limit | Wait for the rate-limit window to reset or use batch operations |
| `71` | `UpgradeProposalNotFound` | Upgrade | Upgrade proposal ID does not exist | Verify `proposal_id` |
| `72` | `UpgradeNotApproved` | Upgrade | Upgrade proposal lacks required admin approvals | Collect required approvals via `approve_upgrade` |
| `73` | `UpgradeAlreadyExecuted` | Upgrade | Upgrade proposal was already executed | Propose a new upgrade version if further changes are needed |
| `74` | `InvalidUpgradeVersion` | Upgrade | New version is not greater than current version | Increment `major`, `minor`, or `patch` above current version |
| `75` | `MigrationFailed` | Upgrade | Storage migration step failed validation | Inspect migration logs and run `simulate_upgrade` |
| `76` | `RollbackWindowExpired` | Upgrade | Rollback window has elapsed since upgrade execution | Deploy a forward-fix upgrade proposal instead |
| `77` | `PermissionDenied` | RBAC | Caller lacks the required `Permission` / `Role` | Grant the required role via `assign_role` from a `SuperAdmin` account |

---

## 5. Complete `contracts/invoice-escrow` Error Code Quick Reference

For full details on `contracts/invoice-escrow` error codes, see [`docs/ERROR_CODES.md`](./ERROR_CODES.md):

| Range | Category | Key Error Codes |
|-------|----------|-----------------|
| `1–99` | Authorization & Access Control | `1` `Unauthorized`, `2` `InsufficientThreshold`, `3` `InvalidSigner`, `4` `DuplicateSigner`, `5` `ThresholdTooHigh`, `6` `ThresholdZero`, `7` `InvalidRole`, `8` `RoleAlreadyAssigned` |
| `100–199` | Validation Errors | `100` `InvalidAmount`, `101` `EmptyParticipants`, `102` `ParticipantLimitExceeded`, `103` `SharesMismatch`, `104` `InvalidDeadline`, `105` `DeadlineExpired`, `106` `InvalidToken`, `107` `InvalidFeeBps`, `108` `StringTooLong`, `109` `InvalidInvoiceId`, `110` `InvalidMetadata` |
| `200–299` | State Errors | `200` `InvoiceNotFound`, `201` `InvalidInvoiceStatus`, `202` `InvoiceAlreadyFunded`, `203` `InvoiceAlreadyReleased`, `204` `InvoiceAlreadyRefunded`, `205` `InvoiceCancelled`, `206` `EscrowNotFound`, `207` `AlreadyPaid`, `208` `InsufficientEscrowBalance`, `209` `DisputeAlreadyOpen`, `210` `DisputeNotFound`, `211` `DisputeAlreadyResolved`, `212` `ContractPaused`, `213` `NotInitialized`, `214` `AlreadyInitialized` |
| `300–399` | Payment & Token Errors | `300` `TransferFailed`, `301` `InsufficientBalance`, `302` `InsufficientAllowance`, `303` `PaymentExceedsOwed`, `304` `UnderpaymentNotAllowed`, `305` `RefundFailed`, `306` `FeeTransferFailed`, `307` `UnsupportedToken`, `308` `SlippageExceeded` |
| `400–499` | Arithmetic Errors | `400` `Overflow`, `401` `Underflow`, `402` `DivisionByZero`, `403` `RoundingError` |
| `500–599` | Storage & Capacity Errors | `500` `StorageFull`, `501` `BatchSizeExceeded`, `502` `RateLimitExceeded`, `503` `DuplicateEntry`, `504` `MigrationVersionMismatch`, `505` `TtlExtensionFailed` |

---

## 6. Frequently Asked Questions (FAQ)

### Q1: Why does `pay` automatically release funds sometimes, and other times the invoice stays `Funded`?
**A**: When a payment brings `invoice.total_paid >= invoice.total_amount`, `pay` checks whether any conditional release rules (`ReleaseCondition`) or open disputes exist on the invoice. If no extra conditions exist and the invoice is not disputed, StellarSplit automatically executes the payout and transitions the invoice to `Released`. If release conditions exist, the invoice transitions to `Funded` until those conditions are satisfied and `release(env, invoice_id)` is called.

### Q2: Can anyone call `release(env, invoice_id)` or `refund(env, invoice_id)`?
**A**: Yes. Both `release` (once fully funded and all conditions are met) and `refund` (once `env.ledger().timestamp() > invoice.deadline` and not fully funded) are permissionless settlement cranks. Because the recipients, payers, and amounts are deterministically fixed in contract storage, any user or automation bot can trigger settlement safely.

### Q3: What happens if an invoice is partially paid and the deadline passes?
**A**: Once `env.ledger().timestamp() > invoice.deadline`, calling `refund(env, invoice_id)` iterates over all recorded contributions (`payer_contributions`) and transfers each payer's exact contributed token amount back to their address, transitioning the invoice to `InvoiceStatus::Refunded`.

### Q4: How do I recover if a recipient's Stellar account cannot receive tokens during push `release`?
**A**: StellarSplit supports pull-payment escrow (`release_to_escrow` + `claim_payout`). Instead of pushing transfers to all recipients in a single transaction (where one frozen trustline could revert the batch), funds can be credited to per-recipient claimable balances (`claimable_key`) and claimed individually via `claim_payout(env, recipient, token)`.

### Q5: How do I check if an invoice's accounting is 100% consistent on-chain?
**A**: Call `verify_invoice_invariant(env, invoice_id)` or `run_invoice_audit(env, invoice_id)`. These read-only functions verify that `total_paid` equals the sum of individual `payer_contributions`, that recipient shares sum to `total_amount`, and that no balance discrepancy exists.

### Q6: Why did my local `cargo clippy` or `cargo test` fail with `rustc` version errors?
**A**: The workspace `Cargo.lock` pins `soroban-sdk 28.0.0`, which requires `rustc >= 1.91.0`. Run Cargo with `RUSTUP_TOOLCHAIN=1.94.1 cargo test --workspace`.
