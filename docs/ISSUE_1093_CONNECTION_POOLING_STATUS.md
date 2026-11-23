# Issue #1093: Connection pooling optimization (status)

**Status:** this repo cannot hold the connections the issue describes. The closest code is a generic
slot pool.

## Why

The issue asks to "optimize Horizon RPC calls with connection reuse". A Soroban contract is `no_std`
and cannot make network calls, and this repository contains no Horizon or RPC client. Connection reuse
for Horizon belongs in whatever service or SDK makes those calls (for example the StellarSplit backend
or frontend), not in `split-contracts`.

## What exists

`contracts/split/src/pool.rs` (issue #1020) is a fixed-size pool of reusable execution slots:

| Requirement | Where it is |
|---|---|
| Pool design and sizing | `PoolConfig` (`capacity`, standby slots, `max_age`, `max_uses`, `failure_threshold`) validated by `Pool::new` |
| Pooling logic | `acquire`, `release(index, ok)` and `recycle` |
| Metrics and connection stats | `Pool::stats` returns `PoolStats`; `Pool::health_check` returns a `HealthReport` |

It is heap free, counts time in ledger sequence numbers, and is declared in `lib.rs` as `pub mod pool;`.
It models slots, not sockets, and no contract entry point uses it yet.

## What this change does

Records the above so the issue can be redirected to the repository that makes the RPC calls. It changes
no code. Nothing was run and no tests were added.
