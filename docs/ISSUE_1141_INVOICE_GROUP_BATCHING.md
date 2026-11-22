# Issue #1141: Invoice group batching

**Status:** core group creation is implemented; batch-release and batch-status
query are partial; several edge cases are undocumented and untested.

---

## Goal

Allow a creator to link a set of invoices into a named group and enforce a
collective funding policy: either every invoice in the group must reach its
target before any single invoice can release its funds (AllOrNothing), or a
strict majority must be fully funded (Majority). Both modes should also support
a single batch-release call that settles all eligible members atomically.

---

## Background: existing implementation

The following is already in `lib.rs`:

```rust
pub fn create_invoice_group(env, invoice_ids: Vec<u64>, majority: bool) -> u64
pub fn get_group_members_list(env, group_id: u64) -> Vec<u64>
```

`create_invoice_group` writes:
- `invoice_group_key(invoice_id) → group_id` for each member (persistent).
- `group_key(group_id) → InvoiceGroup { invoice_ids, mode }` (persistent).
- A monotonic `grp_cnt` counter in persistent storage for group ID assignment.

The payment path (`_pay`) checks `invoice_group_key` and sets `guarded = true`
when the invoice is in a group, suppressing auto-release. This is the only
integration point between groups and the release logic.

`types.rs` defines:

```rust
pub enum GroupMode { AllOrNothing, Majority }
pub struct InvoiceGroup { pub invoice_ids: Vec<u64>, pub mode: GroupMode }
```

---

## What is missing

### 1. No batch-release entry point

There is no public function that iterates a group and releases every eligible
member in one call. The release entry point (`release`) releases a single
invoice; callers must loop themselves, issuing one transaction per member.

**Required:** a `release_invoice_group(group_id)` entry point that:
1. Loads the group.
2. Checks the group's funding policy (AllOrNothing or Majority).
3. If the policy is satisfied, calls the internal `_release` for each member
   whose status is `Pending`.
4. Returns a summary of released and skipped member IDs.

### 2. No group-level funding status query

`get_group_members_list` returns IDs but not funding state. A client wanting
to know whether the group policy is met must load each invoice individually.

**Required:** a `get_invoice_group_status(group_id)` entry point returning:

```rust
pub struct InvoiceGroupStatus {
    pub group_id: u64,
    pub mode: GroupMode,
    pub total_members: u32,
    pub fully_funded_count: u32,
    pub policy_met: bool,
    pub members: Vec<GroupMemberStatus>,
}

pub struct GroupMemberStatus {
    pub invoice_id: u64,
    pub status: InvoiceStatus,
    pub funded: i128,
    pub total: i128,
    pub fully_funded: bool,
}
```

### 3. AllOrNothing policy not enforced at release time

`release` checks `invoice_group_key` only to determine whether auto-release is
blocked on `_pay`. It does not re-check the group policy when `release` is
called manually. A caller can therefore release one member of an AllOrNothing
group before the others are funded.

**Required:** `_release` (or `release`) must load the group membership for the
invoice being released, re-evaluate the policy, and panic with
`ContractError::GroupPolicyNotMet` if the policy is not satisfied.

### 4. No removal or update path

Once an invoice is added to a group it cannot be removed. If a member is
cancelled or expires, the group is permanently stuck: AllOrNothing can never
be satisfied, and Majority may no longer be calculable.

**Required:** either
- a `remove_from_invoice_group(invoice_id)` entry point (creator auth), or
- documented behaviour that a cancelled/expired member is excluded from the
  policy count (i.e. policy is computed over non-terminal members only).

The simpler path is the second: document and implement that a terminal member
(Cancelled, Expired, Refunded, Released) is excluded from the denominator when
computing Majority, and that AllOrNothing ignores terminal members.

### 5. No creator auth on `create_invoice_group`

`create_invoice_group` currently requires no authentication. Any caller can
group any invoices they know the IDs of.

**Required:** verify that the caller is the creator of every invoice in the
group, or is an admin. If the creator requirement cannot be enforced cheaply
(it requires loading each invoice), at minimum document the security model and
add an event so the creator is notified.

### 6. Missing event

No event is emitted when a group is created or when a batch release runs.

**Required:**
- `("split", "grp_crt")` with data `(group_id, member_count, mode_as_bool)` on
  `create_invoice_group`.
- `("split", "grp_rel")` with data `(group_id, released_count, skipped_count)`
  on `release_invoice_group`.

---

## Proposed new types

```rust
/// Issue #1141: Per-member funding state inside a group status query.
#[contracttype]
pub struct GroupMemberStatus {
    pub invoice_id: u64,
    pub status: InvoiceStatus,
    pub funded: i128,
    pub total: i128,
    pub fully_funded: bool,
}

/// Issue #1141: Group-level aggregate returned by get_invoice_group_status.
#[contracttype]
pub struct InvoiceGroupStatus {
    pub group_id: u64,
    pub mode: GroupMode,
    pub total_members: u32,
    pub fully_funded_count: u32,
    pub policy_met: bool,
    pub members: Vec<GroupMemberStatus>,
}

/// Issue #1141: Summary returned by release_invoice_group.
#[contracttype]
pub struct GroupReleaseResult {
    pub group_id: u64,
    pub released: Vec<u64>,
    pub skipped: Vec<u64>,
}
```

---

## Proposed entry points

| Function | Access | Description |
|---|---|---|
| `create_invoice_group(invoice_ids, majority)` | creator of all members | Existing. Add creator auth and `grp_crt` event. |
| `release_invoice_group(creator, group_id)` | creator | Release all Pending members if policy is met. Returns `GroupReleaseResult`. |
| `get_invoice_group_status(group_id)` | anyone | Return `InvoiceGroupStatus`. |
| `get_group_members_list(group_id)` | anyone | Existing. |

---

## Storage

No new storage keys. The required data is already in:
- `group_key(group_id)` → `InvoiceGroup`
- `invoice_group_key(invoice_id)` → `group_id`
- Per-invoice keys read by `load_invoice`

A new `StorageKey` or `AddressKey` variant is not needed.

`grp_cnt` currently uses an inline `symbol_short!("grp_cnt")` literal rather
than a typed `StorageKey` enum variant. This should be migrated to
`StorageKey::GroupCounter` (append to enum 1) to maintain the collision-safety
guarantee of the typed key registry.

---

## Compute budget

`release_invoice_group` and `get_invoice_group_status` loop over all group
members. The group size is bounded only by the length of the `invoice_ids` Vec
passed at creation time, which has no documented upper limit.

**Required:** add a `MAX_GROUP_SIZE` constant (suggested: 20) and enforce it
in `create_invoice_group`. Document the bound in this file and in `API.md`.

---

## Open questions

- Whether `release_invoice_group` should release members sequentially (simpler,
  one failed transfer aborts the rest) or use `try_invoke_contract` per member
  so a single failed transfer does not revert the entire batch. The existing
  batch payment logic (`create_batch` / `pool_pay`) uses a fail-fast approach.
  Group release should document which model it follows.
- Whether the `majority` boolean in `create_invoice_group` should be extended
  to a `threshold_bps` (e.g. 7 500 = 75%) for more flexible quorum rules, or
  kept as a binary AllOrNothing / Majority choice.

---

## Not done

`release_invoice_group`, `get_invoice_group_status`, creator auth on
`create_invoice_group`, the `MAX_GROUP_SIZE` guard, the new events, and the
migration of `grp_cnt` to a typed key are all to be implemented.
