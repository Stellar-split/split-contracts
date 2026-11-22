# Issue #923: Circuit Breaker Pattern

Extends the emergency circuit breaker from issue #297 into a full state machine with an
exemption list, recovery procedure, automatic activation conditions and a transition log.

## State machine

```
            activate / failure threshold reached
  Closed ───────────────────────────────────────▶ Open
    ▲                                             │  ▲
    │ complete_circuit_recovery                   │  │ failure reported
    │ (after recovery_ledgers)                    │  │ during probation
    │                                             ▼  │
    └──────────────────────────────────────── HalfOpen
                     begin_circuit_recovery ◀─┘

  deactivate_circuit_breaker: any state ──▶ Closed (emergency override)
```

| State | Guarded mutating operations |
|-------|-----------------------------|
| `Closed` | Allowed |
| `Open` | Blocked with `ContractPaused`, except exempt operations |
| `HalfOpen` | Allowed; any reported failure re-opens the breaker |

The issue #297 `cb_flag` stays in sync (`true` only in `Open`), so every existing
`require_not_paused` / `check_not_paused` guard keeps working unchanged.

## Admin functions

| Function | Role | Transition |
|----------|------|------------|
| `activate_circuit_breaker(admin, reason)` | SuperAdmin | any → `Open` |
| `deactivate_circuit_breaker(admin)` | SuperAdmin | any → `Closed` |
| `begin_circuit_recovery(admin)` | SuperAdmin | `Open` → `HalfOpen` |
| `complete_circuit_recovery(admin)` | SuperAdmin | `HalfOpen` → `Closed`, once `recovery_ledgers` have passed |
| `set_circuit_breaker_config(admin, config)` | SuperAdmin | n/a |
| `set_circuit_breaker_exemption(admin, function, exempt)` | SuperAdmin | n/a |
| `report_circuit_failure(reporter, reason)` | Operator+ | see below |

Invalid transitions panic with `"invalid circuit breaker transition"`. Completing recovery
too early panics with `"recovery period not elapsed"`.

## Activation conditions

`report_circuit_failure` counts failures in a rolling window of `failure_window_ledgers`:

- `Closed`: the breaker opens when the count reaches `failure_threshold` (`0` turns auto-trip off).
- `HalfOpen`: a single report re-opens the breaker.
- `Open`: the report is counted and emitted only.

Defaults: `failure_threshold = 3`, `failure_window_ledgers = 720` (~1 h),
`recovery_ledgers = 120` (~10 min).

## Exemptions

Exemptions are keyed by the guard name an entry point passes to `require_fn_not_paused`:
`pay`, `pay_tok`, `brg_pay`, `release` or `refund`. For example, exempting `refund` keeps
`refund` and `claim_refund` working while the breaker is open, so payers can still recover
funds. Entry points that use the global guard (`require_not_paused`), and `create_invoice`,
are never exempt.

## Monitoring

- `get_circuit_breaker_status()` returns `{ active, reason, state, opened_at_ledger, recovery_started_ledger, failure_count }`.
- `get_circuit_breaker_history()` returns the last 50 `CircuitBreakerTransition { from, to, actor, reason, ledger, timestamp }` entries.
- `get_circuit_breaker_config()`, `get_circuit_breaker_exemptions()`

## Events

- `cb_state`: every transition `(from, to, actor, reason, ledger)`
- `cb_fail`: failure reported `(failure_count, failure_threshold)`
- `cb_exmpt`: exemption changed `(function, exempt)`
- `cb_act` / `cb_dact` (existing): breaker opened / closed
