# Issue #922: Fee Waiver System

Extends the per-creator fee waiver whitelist from issue #296 with expiry, waiver records and an audit log.

## Storage

| Key | Tier | Type | Purpose |
|-----|------|------|---------|
| `cr_fw` | persistent | `Vec<Address>` | Whitelist index (max 100 entries, from #296) |
| `(fw_rec, creator)` | persistent | `FeeWaiver` | Grantor, grant time, expiry, reason |
| `fw_audit` | persistent | `Vec<FeeWaiverAuditEntry>` | Audit log, capped at 200 entries (oldest dropped) |

`FeeWaiver.expires_at` is a ledger timestamp; `0` means the waiver never expires. Whitelist
entries created before this change have no record and are treated as non-expiring.

## Admin controls (SuperAdmin only)

- `add_fee_waiver(admin, creator)`: grants a non-expiring waiver. Does nothing if the creator already has an active waiver.
- `grant_fee_waiver(admin, creator, expires_at, reason)`: grants or renews a waiver with an expiry.
- `extend_fee_waiver(admin, creator, expires_at)`: changes the expiry of an active waiver.
- `remove_fee_waiver(admin, creator)`: revokes the waiver and deletes its record.

An expiry that is not in the future is rejected with `"waiver expiry must be in the future"`.

## Expiration

- A waiver is active only while `expires_at == 0 || now < expires_at`.
- `purge_expired_fee_waivers(caller) -> u32` is permissionless. It removes expired waivers from the whitelist and returns how many it removed.
- Granting a waiver purges expired entries first, so lapsed waivers stop counting against the 100-entry cap.

## Fee checks

Release (`_release_full`, `_release_tranches`) uses `creator_has_active_fee_waiver`, so an expired waiver no longer zeroes the platform fee, even before it is purged.

## Read functions

- `has_fee_waiver(creator) -> bool`: true only for active waivers.
- `get_fee_waiver(creator) -> Option<FeeWaiver>`
- `list_fee_waivers() -> Vec<Address>`: active waivers only.
- `get_fee_waiver_audit_log() -> Vec<FeeWaiverAuditEntry>`

## Audit log and events

Every grant, extension, revocation and expiry purge appends a `FeeWaiverAuditEntry`
`{ action, creator, actor, timestamp, ledger, expires_at }`.

Events: `fw_grant`, `fw_rev` (existing), `fw_exp` (expiry set or extended), `fw_purge` (expired waiver removed).
