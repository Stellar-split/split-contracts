# Issue #999: Subscription billing (design scaffold)

**Status:** design only. No subscription logic is implemented.

## Goal

A payer subscribes to a recurring invoice. Each period, an invoice is created for the payer and the
subscriber's payment is collected, until the subscription is cancelled.

## Proposed model

- A plan: amount per period, period length in seconds, and the recipient list.
- A subscription: plan ID, payer, start time, and the period number it has reached.
- A billing step, callable by anyone: if a period is due, create one invoice for it. The billing step
  is idempotent per period, so a second call in the same period creates nothing.

## Constraints

- Soroban cannot run code on a timer, so billing happens when someone calls it. Each period must be
  billable late, and the number of catch-up periods must be bounded so one call cannot loop without
  limit.
- The payer must authorise payments. A subscription needs a standing approval, which a token allowance
  or a stored authorisation would provide. This is not decided.

## Open questions

- How the payer's standing approval is held and checked.
- Whether a missed period is billed late or skipped.

## Not done

No code, no tests, no billing step.
