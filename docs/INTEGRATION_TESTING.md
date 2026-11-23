# Integration testing guide

How the tests in this repository are laid out, how to write a test that drives the `split` contract
through a full flow, and how to run and debug it.

## Where tests live

| Location | What it holds | Run by `cargo test --workspace` |
|---|---|---|
| `contracts/split/src/test.rs` | The main suite, about 350 tests that call the contract through its generated client. | Yes |
| `contracts/split/src/boundary_tests.rs`, `fuzz_tests.rs` | Boundary values, and property tests with `proptest`. | Yes |
| `contracts/split/src/testing_support.rs` | Shared helpers: `setup`, `single_invoice`, `multi_invoice`, `try_pay`, `fund`. | Used by the suites above |
| `contracts/split/tests/payment_split_properties.rs` | `proptest` properties for percentage splits. They test the arithmetic on plain vectors and make no contract calls. | Yes |
| `contracts/<crate>/src/test.rs` | Tests for `charity-split`, `dashboard`, `invoice-escrow`, `mentorship`, `skill-verification`, `tax-strategy`. | Yes |
| `contracts/split/test_snapshots/` | JSON snapshots the SDK writes for each test. | Written by the tests |
| `tests/` at the repository root | Cross-contract and lifecycle tests: `e2e_lifecycle.rs`, `invoice_lifecycle.rs`, `event_log_tests.rs`, `helpers.rs` and more. | **No**, see below |
| `fuzz/` | `cargo fuzz` targets with seed corpora. | By the `fuzz` job in CI |

The CI file `.github/workflows/test.yml` runs `cargo test --workspace` and then
`cargo test -p split storage_snapshot`.

### The root `tests/` directory

The root `Cargo.toml` is a virtual workspace: it has no `[package]`, so Cargo does not build the files in
`tests/` at the root as an integration test target. They are also written against a different layout than the
workspace has. `tests/helpers.rs` imports `target/wasm32-unknown-unknown/release/split_contracts.wasm`, and
`event_log_tests.rs` uses a `split_contracts` crate, while the contract package is named `split`.
Treat those files as examples to port, and put new integration tests in `contracts/split/tests/` or in
`contracts/split/src/`, where `cargo test --workspace` runs them.

## The test environment

Every test starts from a fresh `Env`. This is the setup the suite uses (`testing_support.rs`):

```rust
let env = Env::default();
env.mock_all_auths();                       // accept every require_auth()

let contract_id = env.register(SplitContract, ());
let token_admin = Address::generate(&env);
let token_id = env
    .register_stellar_asset_contract_v2(token_admin.clone())
    .address();
StellarAssetClient::new(&env, &token_id).mint(&token_admin, &1_000_000_000);

let c = SplitContractClient::new(&env, &contract_id);
let admin = Address::generate(&env);
let treasury = Address::generate(&env);
c.initialize(&admin, &0_i128, &treasury, &token_id, &0_u32, &None, &0_u32, &0_u32, &0_u64);

env.ledger().set_timestamp(1_000);          // tests control ledger time
```

- `mock_all_auths()` makes every authorisation pass. Use it for flow tests. To test that a call is refused
  for the wrong signer, use `env.mock_auths(...)` with an explicit `MockAuth`, as `tests/helpers.rs` shows.
- `env.ledger().set_timestamp(..)` and `set_sequence_number(..)` move time. Deadline, refund and archival
  behaviour depend on them.
- Fund each payer with `StellarAssetClient::new(&env, &token_id).mint(&payer, &amount)` before it pays.

## A full-flow test

This test creates an invoice, pays it and checks the release. It follows `test_pay_and_auto_release` in
`contracts/split/src/test.rs`:

```rust
#[test]
fn test_pay_and_auto_release() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128, &0_u64, &false, &false, &None);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);
}
```

`pay` takes the payer, the invoice id, the amount, a nonce (it must equal `get_nonce(invoice_id, payer)`, so
`0` for a first payment), two flags and an optional value. `make_invoice` builds a one-recipient invoice with
`default_options`.

## Testing failures

- **A panic with a message:** `#[should_panic(expected = "invoice deadline has passed")]`.
- **A typed error:** call the `try_` variant of the client method and match the error:
  `c.try_pay(...)` returns a `Result`, and `testing_support::try_pay` wraps it as a `bool`.
- **A multi-step flow:** assert the status and balances after each step, not only at the end, so a failure
  points at the step that broke.

The panic messages and error codes are listed in [`ERROR_CODES_SPLIT.md`](./ERROR_CODES_SPLIT.md).

## Checking events

Import the `Events` trait and read `env.events().all()` after the call:

```rust
use soroban_sdk::testutils::Events as _;
let events = env.events().all();
```

`contracts/split/src/test.rs` has examples that count events and scan `env.events().all().events()` for a
match. Most events have the topics `(split, <name>, invoice_id)`, see
[`ISSUE_1107_EVENT_FILTERING_STATUS.md`](./ISSUE_1107_EVENT_FILTERING_STATUS.md).

## Templates and tips

- Write one behaviour per test and name it for the behaviour, for example `test_pay_after_deadline_panics`.
- Put shared set-up in a helper, as `testing_support.rs` does, not in a copy for each test.
- Do not read `test_snapshots/*.json` to decide whether a test passed. They record what happened and are
  rewritten when the test runs. Review the diff if a snapshot changes in your PR.
- When you add or rename a storage key, `cargo test -p split storage_snapshot` fails on purpose. The steps to
  update the baseline are in [`ARCHITECTURE.md`](./ARCHITECTURE.md#34-storage-key-snapshot).

## Running and debugging

```bash
cargo test --workspace                       # everything CI runs first
cargo test -p split test_pay_and_auto_release # one test by name
cargo test -p split -- --nocapture            # show println! output
RUST_BACKTRACE=1 cargo test -p split <name>   # backtrace for a panic
```

If a test fails with `MigrationRequired`, the contract in the test was left at an older schema version.
Call `migrate` as a SuperAdmin first, or initialize through `initialize`, which stamps the current version.
