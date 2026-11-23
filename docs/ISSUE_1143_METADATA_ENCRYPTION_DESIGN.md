# Issue #1143: Invoice metadata encryption

**Status:** design only. This document also records security problems in the existing encryption
entry points (issue #1012) that should be fixed before any encrypted metadata is stored.

## Goal

Creators can attach sensitive invoice details (customer names, addresses, tax ids, contract terms) to
an invoice without making them readable by everyone. Authorised parties can decrypt them, keys can be
rotated without losing old data, and every access to a key is recorded.

## The constraint that shapes this design

**Everything a Soroban contract stores, receives as an argument or emits is public.** Ledger entries,
transaction arguments and events can all be read by any node operator or indexer. So:

- The contract must never receive plaintext or a decryption key.
- The contract cannot decrypt anything, because the key would have to be on-chain.
- Encryption and decryption happen in the client. The contract stores ciphertext, records which key
  version was used, controls who may write and rotate, and keeps the audit trail.

The existing `confidential_tracking.rs` module (issue #295) already follows this model for
`encrypted_amount`: it treats the value as opaque bytes, limits its size and stores its SHA-256.

## Relevant existing code

| Item | Location | Notes |
|---|---|---|
| Per-invoice metadata store (issue #920) | `contracts/split/src/invoice_metadata.rs` | Keys `[a-z0-9_.-]` up to 32 bytes, values up to 256 bytes, 20 entries, creator or co-creator write, lock flag, per-entry `version`. |
| `metadata_hash` on `Invoice` (issue #329) | `types.rs` | A single hash of off-chain metadata. |
| Encryption entry points (issue #1012) | `lib.rs`, "Issue #1012: Encryption" | `set_encryption_key`, `rotate_encryption_key`, `encrypt_data`, `decrypt_data`, `validate_encryption`, `get_encryption_audit`. |
| `EncryptionKey`, `EncryptionAuditEntry` | `types.rs` | |

### Problems in the issue #1012 entry points

These are a data-leak risk, and the acceptance criteria include "no data leaks".

1. **Raw key material is passed on-chain.** `set_encryption_key` and `rotate_encryption_key` take
   `key_data: BytesN<32>` as a transaction argument and put it in `EncryptionKey.key_data`. Anyone can
   read the key from the transaction, whether or not it is saved.
2. **`encrypt_data` and `decrypt_data` return their input unchanged** (marked as stubs in the code).
   A client calling `encrypt_data` publishes its plaintext in the transaction and gets the plaintext
   back.
3. **The storage and audit helpers do nothing.** `save_encryption_key` and `append_encryption_audit`
   have empty bodies. `load_encryption_key` returns a zero key with `active: true` for any version.
   The audit trail is therefore always empty, and `validate_encryption` checks a key that was never
   stored.
4. **One contract-wide key.** Every invoice shares one admin-controlled key, so the admin can decrypt
   every creator's data.

Recommendation: deprecate `encrypt_data` and `decrypt_data` (make them panic with a clear message),
and stop accepting `key_data` in the key functions. The design below replaces them.

## Proposed design

### Scheme: envelope encryption, done in the client

For each invoice:

1. The client generates a random 256-bit **data encryption key (DEK)**.
2. Each metadata value is encrypted with the DEK using XChaCha20-Poly1305 (a 24-byte random nonce
   makes nonce reuse unlikely). The associated data is `invoice_id || key || dek_version`, so a
   ciphertext cannot be moved to another invoice or key without failing authentication.
3. The DEK is **wrapped** (encrypted) for each authorised reader, using that reader's X25519 public
   key (sealed box). Readers are the creator, any co-creators, and optionally an auditor.
4. The contract stores the ciphertexts, the wrapped DEKs and the public key fingerprints. It never
   sees the DEK or any private key.

### Key registry

Readers publish **public** keys only:

| Key | Value | Storage |
|---|---|---|
| `(enc_pk, address)` | `EncPublicKey { x25519: BytesN<32>, version: u32, registered_at: u64, revoked: bool }` | persistent |

`register_encryption_public_key(owner, pubkey)` requires `owner.require_auth()`. Registering a new
key increases `version` and keeps the old version's record, marked revoked, so old wrapped DEKs can
still be identified.

### Encrypted metadata entries

Encrypted values use the existing `invoice_metadata.rs` store. A new entry type sits alongside
`MetadataEntry`:

```rust
#[contracttype]
pub struct EncryptedMetadataEntry {
    pub key: String,                 // plaintext label, same rules as #920; must not contain secrets
    pub ciphertext: Bytes,           // nonce || ciphertext || tag
    pub ciphertext_hash: BytesN<32>, // SHA-256, computed by the contract
    pub dek_version: u32,
    pub version: u32,
    pub updated_by: Address,
    pub updated_at: u64,
}
```

| Key | Value | Storage |
|---|---|---|
| `(imeta_enc, invoice_id, key)` | `EncryptedMetadataEntry` | persistent |
| `(imeta_dek, invoice_id, dek_version, reader)` | `Bytes` wrapped DEK | persistent |
| `(imeta_dkv, invoice_id)` | `u32` current DEK version | persistent |
| `(imeta_eau, invoice_id)` | `Vec<EncryptionAuditEntry>`, capped length | persistent |

Limits: ciphertext at most 512 bytes (256 plaintext bytes plus overhead, matching
`MAX_ENCRYPTED_AMOUNT_LEN`), and at most 8 readers per invoice.

The encrypted index is separate from the plaintext index, so the 20-entry limit applies to each.
The existing lock flag (`imeta_lck`) applies to both.

### Entry points

| Function | Auth | Effect |
|---|---|---|
| `set_encrypted_metadata(caller, invoice_id, key, ciphertext, dek_version)` | creator or co-creator | Stores the entry. Rejects a `dek_version` that is not current. |
| `set_metadata_deks(caller, invoice_id, dek_version, wrapped: Map<Address, Bytes>)` | creator or co-creator | Stores wrapped DEKs. Every reader must have a registered, unrevoked public key. |
| `get_encrypted_metadata(invoice_id, key)` | none | Returns the ciphertext entry. Public anyway; only key holders can decrypt it. |
| `get_metadata_dek(invoice_id, dek_version, reader)` | none | Returns the wrapped DEK for that reader. |
| `rotate_metadata_dek(caller, invoice_id, wrapped)` | creator or co-creator | Increases the DEK version and stores the new wrapped DEKs (see below). |
| `record_metadata_access(reader, invoice_id, key)` | `reader` | Optional audit record of a read. |
| `get_metadata_encryption_audit(invoice_id, from, limit)` | none | Pages through the audit trail. |

"Decryption" in the acceptance criteria is a client operation: fetch the wrapped DEK, unwrap it with
the reader's private key, fetch the ciphertext, decrypt it. A client SDK helper should do this, with
a round-trip test.

### Key rotation

Two levels of rotation:

- **Reader key rotation** (lost or replaced device): the reader registers a new public key. The
  creator re-wraps the current DEK for that reader with `set_metadata_deks`. No ciphertext changes.
- **DEK rotation** (a reader is removed, or the DEK may be compromised):
  1. The client generates a new DEK and re-encrypts every value.
  2. The client calls `rotate_metadata_dek` with the new wrapped DEKs, then `set_encrypted_metadata`
     for each value with the new `dek_version`. Up to 10 values fit in one batch call, mirroring
     `MAX_METADATA_BATCH`.
  3. Until every entry is on the new version, entries keep their own `dek_version` and the old
     wrapped DEKs stay readable. A rotation is never left half-readable.
  4. After all entries are on the new version, the old wrapped DEKs can be removed.

The safety rule: the contract rejects a write whose `dek_version` is older than the current version,
and it never deletes a DEK version that an entry still references.

Rotation cannot revoke access to data a removed reader has already decrypted, and old ciphertext
remains in ledger history. This limitation must be stated in user-facing documentation.

### Audit trail

Every write appends an `EncryptionAuditEntry`, reusing the existing type:

| `action` | When |
|---|---|
| `enc_set` | an encrypted value is written |
| `dek_set` | wrapped DEKs are added for readers |
| `dek_rot` | the DEK version increases |
| `pk_reg` | a reader registers or replaces a public key (stored on the reader's own trail) |
| `access` | a reader calls `record_metadata_access` |

`data_hash` holds the ciphertext hash, or the hash of the wrapped DEK. The trail is capped (for
example the last 100 entries per invoice) to bound storage. Each entry is also emitted as an event,
and the event stream is the complete audit record.

A contract cannot see who reads public data, so read auditing is opt-in. `record_metadata_access`
records only what honest clients report.

## What must never appear on-chain

- Plaintext metadata values.
- A DEK or any private key, in any form other than wrapped for a reader's public key.
- Secret information in the plaintext `key` labels. Use `tax_id`, not `tax_id_12-345`.

## Test plan

| Area | Test |
|---|---|
| Encryption | A client SDK round trip (encrypt, store, fetch, unwrap, decrypt) returns the original bytes. |
| Integrity | A ciphertext copied to another invoice or key fails to decrypt (associated data check). |
| Access | A stranger cannot write; a co-creator can; a locked store rejects encrypted writes. |
| Access | `set_metadata_deks` for a reader with no registered key panics. |
| Rotation | After DEK rotation, entries on both versions decrypt until migration finishes. |
| Rotation | A write with an old `dek_version` panics. |
| Rotation | Deleting a DEK version still referenced by an entry panics. |
| Audit | Every write adds exactly one audit entry with the correct action and hash; the cap drops the oldest entry. |
| Leaks | No entry point accepts a raw 32-byte symmetric key. |
| Leaks | `encrypt_data` and `decrypt_data` panic as deprecated. |
| Limits | A 513-byte ciphertext and a 9th reader are both rejected. |

## Open questions

- Should an auditor or platform reader be required, optional, or not allowed?
- Should the plaintext key labels themselves be optionally hidden, using an HMAC of the label?
- Should encrypted metadata be deleted on invoice archival (`ISSUE_977_INVOICE_ARCHIVAL_STATUS.md`)?
- Which client library provides the reference implementation (for example libsodium in the
  frontend)?
