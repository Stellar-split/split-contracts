# Rust testing best practices

Patterns for writing tests in this repository, with templates and a checklist. Every example here is
taken from the existing tests in `contracts/split/src/test.rs` and `testing_support.rs`. For the layout of
the test directories and how to run them, see [`INTEGRATION_TESTING.md`](./INTEGRATION_TESTING.md).

## Principles

1. **One behaviour per test, named for the behaviour.** The suite uses `test_<what happens>`, and
   prefixes the issue number when the test pins an issue: `test_310_execute_upgrade_before_timelock_panics`.
2. **Arrange, act, assert.** Build the state, make one call, then check the result and the side effects
   (status, balances, events).
3. **Control time.** Set the ledger time at the start of the test with `env.ledger().set_timestamp(..)`.
   A test that depends on the real clock is not repeatable.
4. **Check the effect, not only the absence of a panic.** After a payment, assert the invoice status and
   the token balances, as `test_pay_and_auto_release` does.
5. **Test the failure path of every new check.** A new `assert!` or `ContractError` needs a test that
   triggers it, and one that stays just inside the limit.

## Templates

### Shared set-up

Use the helpers, not a copy of the set-up in each test:

```rust
let (env, contract_id, token_id) = setup_initialized();   // Env, contract registered and initialized
let c = client(&env, &contract_id);                        // SplitContractClient
let tk = token_client(&env, &token_id);                    // token client for balances
```

`testing_support.rs` adds `single_invoice`, `multi_invoice`, `fund` and `try_pay` for the state machine,
boundary and performance suites.

### A behaviour test

```rust
#[test]
fn test_create_invoice() {
    let (env, contract_id, token_id) = setup_initialized();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 2_000);
    assert_eq!(id, 1);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.funded, 0);
}
```

### A failure test

Use `should_panic` with the exact message when the check is an `assert!` or `panic!`:

```rust
#[test]
#[should_panic(expected = "upgrade timelock still active")]
fn test_310_execute_upgrade_before_timelock_panics() { /* ... */ }
```

For a typed error, call the `try_` method of the client and match the result, so the test fails if the
call succeeds or fails with a different error:

```rust
assert!(c.try_pay(&payer, &id, &amount, &0_u64, &false, &false, &None).is_err());
```

`expected = "..."` matches a substring of the panic message. Use enough of the text to tell two checks apart.

### A boundary test

Pair every limit with the value just inside it and the value just outside it. For a recipient cap of 200,
test 200 (accepted) and 201 (`BatchLimitExceeded`). `boundary_tests.rs` is the place for these.

### A property test

Use `proptest` for arithmetic that must hold for all inputs, such as splits that add up to the total.
`fuzz_tests.rs` keeps a plain Rust reference model next to each property and compares the two:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn shares_add_up_to_the_total(weights in proptest::collection::vec(0u32..=10_000, 1..=16)) {
        // normalise, then assert the parts sum to 10_000
    }
}
```

Commit the file `proptest` writes in `contracts/split/proptest-regressions/`. It replays the inputs that
once failed, so a fixed bug stays fixed.

## Things to avoid

- **`std` in contract code.** The crates are `#![no_std]`. Test modules that need `std` have to opt in
  with `extern crate std;`, as `fuzz_tests.rs` does. Three suites, `state_machine_tests`,
  `concurrency_tests` and `perf_regression_tests`, are commented out in `lib.rs` with the note "Disabled due
  to std dependencies incompatible with no_std contract". They do not run in CI.
- **Shared state between tests.** Each test makes its own `Env`. Do not keep globals.
- **Asserting on snapshot files.** `test_snapshots/*.json` are written by the SDK. They show what happened,
  they are not the expectation.
- **Large copy-pasted options.** `default_options(&env)` builds the `InvoiceOptions` struct with every field
  set. Start from it and change the one field the test cares about.
- **Testing through `mock_all_auths` only.** It lets every call through. When the point of the test is who may
  call, use `env.mock_auths(...)` with the specific signer.

## Checklist

- [ ] The test name says what is checked.
- [ ] Ledger time is set explicitly.
- [ ] Balances and status are asserted, not only that the call returned.
- [ ] A failure test exists for each new `assert!` or `ContractError`.
- [ ] Both sides of a limit are tested.
- [ ] No test depends on another test or on test order.
- [ ] `cargo test --workspace` passes, and `cargo test -p split storage_snapshot` if a key changed.

## Tips

- Run one test: `cargo test -p split test_create_invoice`.
- See `println!` output: `cargo test -p split <name> -- --nocapture`.
- Get a backtrace for a panic: `RUST_BACKTRACE=1 cargo test -p split <name>`.
- Keep a test short. If set-up takes more than a screen, move it into a helper.
