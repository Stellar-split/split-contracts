# Issue #1147: Invoice metadata versioning (design)

**Status:** design only. Each metadata key already has a version counter (#920), but no history is
kept. This doc describes how to keep old versions so they can be listed and compared.

## What already exists

**Key-value store** in `contracts/split/src/invoice_metadata.rs` (#920):

- Each entry is a `MetadataEntry { key, value, version, updated_by, updated_at }` under
  `(imeta_ent, invoice_id, key)`.
- `version` starts at 1 and goes up by one on each overwrite. The old value is **overwritten**, so it
  is gone.
- `remove_invoice_metadata` and `clear_invoice_metadata` delete the entry. If the key is set again
  later, its version restarts at 1.
- `lock_invoice_metadata` makes the store permanently read-only.
- Limits: 20 keys per invoice, 32-byte keys, 256-byte values, 10 entries per batch.
- Events: `invoice_metadata_set(key, version)`, `invoice_metadata_removed`, `invoice_metadata_locked`.

**Off-chain hash:** `update_metadata_hash` (#329) replaces a single `BytesN<32>` and emits the old
and new hash. The old hash is only in the event.

Since history only exists in events, the current state cannot be rebuilt from contract storage alone.
That is the data loss this issue is about.

## Design

### 1. A version counter per invoice

Each invoice gets one counter, `(imeta_ver, invoice_id) → u32`. It goes up by one on every write,
remove, clear, batch or hash update. A batch counts as **one** version. So a single invoice version
number describes the whole metadata state at that moment, which is what makes comparing two versions
possible.

The `version` on each key stays as it is, for compatibility. It no longer restarts after a remove: the
last version number per key is kept in the history.

### 2. A change record per version

```rust
#[contracttype]
pub struct MetadataChange {
    pub key: String,
    pub old_value: Option<Bytes>,   // None = the key did not exist
    pub new_value: Option<Bytes>,   // None = the key was removed
}

#[contracttype]
pub struct MetadataVersion {
    pub version: u32,
    pub changes: Vec<MetadataChange>,   // 1 entry, or up to MAX_METADATA_BATCH, or 20 for a clear
    pub by: Address,
    pub at: u64,
}
```

Stored under `(imeta_his, invoice_id, version)`, one entry per version, so a single read fetches one
version.

The hash from `update_metadata_hash` is recorded with the reserved key `_hash`. The current key
alphabet (`[a-z0-9_.-]`) allows a leading `_`, so this design adds a new rule: user writes of keys
that start with `_` are refused. Any existing key that starts with `_` can still be read and removed.

### 3. Limit on stored history

Each version takes at most about 20 × 2 × 256 bytes. History is capped at `MAX_METADATA_VERSIONS = 50`
per invoice. When the 51st version is written, the oldest version entry is removed and
`(imeta_base, invoice_id) → u32` records the oldest version still kept. Locking the store stops new
versions, so a locked invoice's history never changes.

"No data loss" therefore means: every version inside the window can be fully rebuilt, and versions
older than that are reported as pruned, not returned as wrong data.

### 4. Queries

| Function | Returns | Cost |
|---|---|---|
| `get_metadata_version(invoice_id)` | The current invoice version | 1 read |
| `get_metadata_change(invoice_id, version)` | `Option<MetadataVersion>` | 1 read |
| `get_metadata_history(invoice_id, start, limit)` | One page of `MetadataVersion`s, `limit ≤ 20` | `limit` reads |
| `get_metadata_at(invoice_id, version)` | `Map<String, Bytes>` as it was at that version | current state + (current − version) reads |
| `get_key_history(invoice_id, key, limit)` | That key's past values, newest first | Up to `MAX_METADATA_VERSIONS` reads |

`get_metadata_at` works backwards from the current state by undoing each change. It refuses versions
older than `imeta_base`.

### 5. Comparison

```rust
pub fn diff_metadata(env, invoice_id, from: u32, to: u32) -> Vec<MetadataChange>
```

Returns one entry per key that is different between the two versions, using `old_value` from `from`
and `new_value` from `to`. A key changed and then changed back is left out. Requires `from ≤ to`, and
both inside the kept window.

### 6. Events

`invoice_metadata_set` and `invoice_metadata_removed` also carry the invoice version. A new
`invoice_metadata_version` event is emitted once per batch or clear.

## Migration

Invoices that already have metadata start at invoice version 0, with no history entries. The first
write after the upgrade creates version 1, and its `old_value` is the value already stored, so nothing
is lost.

## Test plan (`test_1147_*`)

- Each write, batch, remove, clear and hash update raises the invoice version by exactly one.
- `get_metadata_at(v)` equals the state that was saved after write `v`, for every `v` in a sequence of
  30 mixed operations.
- `diff_metadata` reports adds, changes and removes, and leaves out a key that was changed and changed
  back.
- Removing a key and setting it again continues its per-key version instead of restarting at 1.
- After 60 versions only the last 50 can be read; older ones are refused with a clear error.
- A locked store refuses writes and its history no longer changes.
- Writing a key that starts with `_` is refused.
- An invoice that had metadata before the upgrade keeps its old value in the first history entry.

## Acceptance criteria

| Criterion | Met by |
|---|---|
| Versions tracked | Per-invoice version counter; the per-key `version` kept |
| Queries efficient | One read per version; reads are paged and capped |
| Comparison works | `diff_metadata` |
| History complete | Every write, batch, remove, clear and hash update is recorded |
| No data loss | `old_value` stored in each change; versions older than the window are reported, not made up |
| All tests pass | `cargo test -p split test_1147` |

## Open questions

- Is 50 versions per invoice the right cap? In the worst case (every version a full clear) history
  takes about 50 × 20 × 2 × 256 B ≈ 0.5 MB per invoice. Storing 32-byte value hashes instead of
  values would make it up to 8× smaller, but old values could then not be rebuilt.
- Should cancelled or deleted invoices keep their history, or have it removed during purge?
