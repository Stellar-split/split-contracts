# Issue #1153: Test Plan — Metadata Encryption Roundtrip

Test plan only — no test code. Read the warning below before writing any test.

## Status warning: the encryption layer is a stub

The encryption API from issue #1012 exists, but its core is not implemented.
Several of this issue's acceptance criteria cannot pass against the current
code. Here is what the code actually does:

| Function | Location | Actual behaviour |
|---|---|---|
| `encrypt_data` | `contracts/split/src/lib.rs:19398` | Returns `data` **unchanged** (`// Return data as-is (stub)`). Stores `sha256(data) → version`. |
| `decrypt_data` | `contracts/split/src/lib.rs:19431` | Returns input **unchanged**. Panics only if `key_version > current`. |
| `load_encryption_key` | `contracts/split/src/lib.rs:17271` | Ignores storage. Always returns an all-zero key with `active: true`. |
| `save_encryption_key` | `contracts/split/src/lib.rs:17281` | Empty body. Nothing is persisted. |
| `append_encryption_audit` | `contracts/split/src/lib.rs:17283` | Empty body. `get_encryption_audit` always returns `[]`. |
| `events::encryption_key_set` / `data_encrypted` / `data_decrypted` / `encryption_validated` | `contracts/split/src/events.rs:2685-2691` | Empty bodies. No events are emitted. |
| `rotate_encryption_key` | `contracts/split/src/lib.rs:19353` | Increments the version counter. Its attempt to deactivate the old key is lost because save does nothing and load always returns `active: true`. |

What does work: the version counter (`enc_version_key`), the "already set" and
"no key set" guards, admin auth, caller auth, and the
`hash → version` registry used by `validate_encryption`.

"Metadata" does not go through this layer either. `invoice_metadata.rs` and
`metadata_hash` store plaintext or hashes. The only ciphertext in the contract
is `encrypted_amount` in `confidential_tracking.rs`, which the contract treats
as opaque bytes.

**Recommendation:** split the work into two parts.
1. **Now:** write the tests in sections A and B below. They cover behaviour
   that exists and keep it from regressing.
2. **Blocked on implementation:** write the tests in section C as `#[ignore]`d
   tests with the reason `"blocked: #1012 encryption is a stub"`. They become
   the acceptance suite for whoever finishes #1012. Keep this issue open, or
   link it to a follow-up, until they pass.

## Code under test

- Types: `EncryptionAlgorithm`, `EncryptionKey`, `EncryptionAuditEntry`,
  `EncryptionValidation` — `contracts/split/src/types.rs:2414-2470`
- Entry points: `set_encryption_key`, `rotate_encryption_key`, `encrypt_data`,
  `decrypt_data`, `validate_encryption`, `get_encryption_audit`,
  `get_encryption_key_version` — `contracts/split/src/lib.rs:19313-19516`
- Storage keys: `enc_audit_key`, `enc_version_key`, `enc_data_version_key`,
  `current_key_version` — `contracts/split/src/lib.rs:17242-17260`

## A. Behaviour that works today

### Key lifecycle

1. **Fresh contract.** `get_encryption_key_version() == 0`.
2. **`set_encryption_key` sets version 1.** A second call panics with
   `"encryption key already set"`.
3. **`set_encryption_key` without admin auth** fails at `require_admin`.
4. **`rotate_encryption_key` before any key** panics with `"no encryption key set"`.
5. **Rotation increments the version.** Set the key, then rotate three times:
   the version goes 1, 2, 3, 4. Both algorithms
   (`Aes256Gcm`, `ChaCha20Poly1305`) are accepted.
6. **Rotation without admin auth** fails, and the version is unchanged.

### Encrypt and decrypt guards

1. **`encrypt_data` before a key is set** panics with `"no encryption key set"`.
2. **`encrypt_data` without the caller's auth** fails at `caller.require_auth()`.
3. **`decrypt_data` with `key_version > current`** panics with
   `"invalid key version"`.
4. **`decrypt_data` without the caller's auth** fails.

### Integrity registry (`validate_encryption`)

1. **Encrypt then validate at the current version** returns `valid == true`
   and `error == "ok"`.
2. **Validate data that was never encrypted** returns `valid == false` and
   `error == "hash_mismatch"`.
3. **Validate with the wrong version.** Encrypt at v1, rotate to v2, then
   validate at v2: `valid == false`, `"hash_mismatch"`. Validate at v1:
   `valid == true`. This is expected because the old key is never actually
   deactivated; see C.3.
4. **One-byte change.** Encrypt `D`, then validate `D'` (one byte flipped):
   `"hash_mismatch"`.
5. **Validate with `key_version > current`** panics with `"invalid key version"`.
6. **Re-encrypting the same plaintext after rotation** moves its registry entry
   to the new version. The old version then validates as `hash_mismatch`.
   Assert this, because it is a real data-integrity consequence of the
   per-hash registry.

## B. Edge cases (current behaviour)

1. **Empty `Bytes`.** `encrypt_data(&Bytes::new())` succeeds, and the hash of
   empty input is registered. Decide whether empty input should be rejected;
   if so, add a guard and flip this test.
2. **Large payload** (for example 4 KiB, and the instruction budget limit)
   succeeds within budget, or fails cleanly. Record the measured cost.
3. **Binary data with every byte value** (`0x00..=0xFF`) roundtrips unchanged.
   This holds trivially today but should stay true once real encryption lands.
4. **`decrypt_data(key_version = 0)`** currently succeeds, because `0 <= current`
   and the stubbed load returns a key. This is almost certainly wrong. Write
   the test asserting a panic, and mark it `#[ignore]` until a guard is added.

## C. Blocked until #1012 is implemented

Write these as `#[ignore = "blocked: #1012 encryption is a stub"]`.

### Encryption works

1. **Ciphertext is different from plaintext.** `encrypt_data(D) != D` for any
   non-empty `D`.
2. **Non-deterministic with a nonce.** Encrypting the same `D` twice gives
   different ciphertexts. This applies to AES-GCM and ChaCha20-Poly1305 with a
   random or counter nonce.
3. **Ciphertext length is** `len(D) + nonce_len + tag_len` (12 + 16 for both
   algorithms).

### Decryption is accurate

1. **Roundtrip.** `decrypt_data(encrypt_data(D), v) == D` for a table of
   inputs: empty, 1 byte, 31, 32 and 33 bytes (block edges), 1 KiB, UTF-8 JSON
   metadata, and every byte value.
2. **Both algorithms** pass the roundtrip table.

### Rotation is safe

1. **Old data still decrypts after rotation.** Encrypt `D` at v1, rotate to v2,
   then `decrypt_data(ct, 1) == D`.
2. **New data uses the new key.** After rotation, `encrypt_data` records v2 in
   the registry and its ciphertext does not decrypt under v1.
3. **Old key is actually retired.** After rotation, `load_encryption_key(1).active == false`,
   and `validate_encryption(ct_v1, 1)` returns `"key_inactive"`. This is
   currently impossible because `save_encryption_key` does nothing.
4. **Key material is persisted per version.** v1 and v2 load different
   `key_data`. Today both load `[0; 32]`.
5. **Decrypting with the wrong version fails cleanly.** Decrypting v1
   ciphertext with v2 panics or returns an error. It must never return garbage
   as if it were plaintext; the GCM or Poly1305 tag check guarantees this.

### Data is intact

1. **Tamper detection.** Flip one byte of the ciphertext, the tag or the nonce,
   and decryption fails.
2. **Truncated ciphertext** fails.

### Audit and events

1. **`get_encryption_audit`** has one entry per `set_key`, `rotate`,
   `encrypt`, `decrypt` and `validate`, with the correct `actor`,
   `key_version` and `data_hash`. Today it is always empty.
2. **Each operation emits its event.** Today none do.
3. **The audit uses the real caller.** `set_key` and `rotate` currently pass
   `env.current_contract_address()` as the actor, not the admin. Assert the
   admin's address.

## Design question to settle before implementing C

Soroban contracts run on a public ledger. Any key stored in contract storage,
and any plaintext passed as a contract argument, is readable by anyone. On-chain
symmetric encryption with a stored key therefore protects nothing. Before
anyone builds C.1–C.5, the maintainers should decide whether:

- encryption should happen **off-chain**, with the contract only storing
  ciphertext and a key version, as `confidential_tracking.rs` already does with
  `encrypted_amount`; or
- the on-chain API stays as a version and integrity registry only, and
  sections C.1–C.4 are dropped from scope.

That decision changes what "Encryption works" and "Rotation safe" mean for this
issue.

## Acceptance-criteria mapping

| Criterion | Testable today | Blocked |
|---|---|---|
| Encryption works | A (guards) | C "Encryption works" |
| Decryption accurate | B.3 (identity) | C "Decryption is accurate" |
| Rotation safe | A key lifecycle, A.3, A.6 | C "Rotation is safe" |
| Data intact | A.1–A.4 (hash registry) | C "Data is intact" |
| Edge cases handled | B.1–B.4 | — |
| All tests pass | Sections A and B green | Section C `#[ignore]`d with tracked reason |
