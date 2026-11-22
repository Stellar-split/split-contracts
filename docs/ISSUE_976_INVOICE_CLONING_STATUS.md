# Issue #976: Invoice cloning (status)

**Status:** the feature exists. `clone_invoice` and `clone_invoice_partial` in
`contracts/split/src/lib.rs` create a new invoice from an existing one.

## What this change does

Records the status above. It changes no code and adds no tests.

## What still needs checking

- What `clone_invoice` copies and what it resets. The new invoice must start with no payments and its
  own status, not the source's.
- Whether `clone_invoice_partial` copies only the fields its name suggests.
- Whether the clone emits an event that links it to its source.

## Not done

No review of the clone logic, no new tests.
