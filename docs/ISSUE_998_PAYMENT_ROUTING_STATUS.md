# Issue #998: Payment routing and splitting (status)

**Status:** the splitting logic exists in `contracts/split/src/split_rules.rs`, the rules engine
from #913. It computes each recipient's amount from the invoice's live state, and the first matching
rule per recipient decides. Routing an amount to a destination beyond the recipient list is not
covered by that module.

## What this change does

Records the status above. It changes no code and adds no tests.

## What still needs checking

- Whether "routing" in the issue means choosing among recipients (covered by the rules engine) or
  forwarding a payment to another contract (not covered).
- That recipients with no matching rule receive nothing, as the module documents, and that the
  amounts still sum to the funded total.

## Not done

No review of the rules engine, no new routing code.
