# Issue #1121: Invoice metadata encryption (status)

**Status:** not met. An encryption API exists (issue #1012), but it does not encrypt anything, and it is not
connected to invoices. This note lists what the code does, why on-chain encryption cannot give the
confidentiality the issue asks for, and a design that can. No code is changed, because a change here would
have to replace the API and could not be built or tested in this run.

## What the code does today

The functions are in `contracts/split/src/lib.rs`, in the block headed "Issue #1012: Encryption":
`set_encryption_key`, `rotate_encryption_key`, `encrypt_data`, `decrypt_data`, `validate_encryption`,
`get_encryption_audit` and `get_encryption_key_version`.

| Requirement | What is there |
|---|---|
| Encryption scheme | `EncryptionAlgorithm` and `EncryptionKey` types exist. Nothing implements a cipher. |
| Encryption | `encrypt_data` returns its input unchanged. The code says "Stub implementation". It stores `sha256(data) -> key version` and emits an event, so the plaintext goes in the transaction and is public. |
| Decryption | `decrypt_data` also returns its input unchanged. |
| Key rotation | `rotate_encryption_key` calls `save_encryption_key`, which is an empty function, so no key is stored and nothing is deactivated. |
| Audit trail | `append_encryption_audit` is an empty function, so `get_encryption_audit` always returns an empty list. Events such as `data_encrypted` are emitted. |
| Link to invoices | None. No function takes an invoice id, and `InvoiceOptions` has no encrypted field. The invoice's only metadata is a `metadata_hash` (`update_metadata_hash`) and a note. |

Helpers behind them that are placeholders:

- `current_key_version` always returns `1`. So `get_encryption_key_version` always says 1, and
  `set_encryption_key` can never run, because it asserts the version is `0` (`encryption key already set`).
- `load_encryption_key` returns an all-zero key with `active: true` for any version.

Net effect: the API reports success but gives no protection, and the audit trail and key state do not exist. A
caller who trusts `encrypt_data` would publish sensitive data believing it was protected.

## Why the contract cannot do this itself

- Contract storage and transaction arguments are public. A key given to `set_encryption_key` would be readable by
  anyone who reads the ledger, and so would the data that is passed to `encrypt_data`.
- A contract has no secret of its own to decrypt with. The decrypt step has to happen off-chain, with a key
  the contract never sees.

So the contract should store ciphertext that was made off-chain, and keep track of which key version made it.

## A design that works

1. **Encrypt off-chain.** The creator encrypts the metadata with an AEAD cipher such as AES-256-GCM or
   XChaCha20-Poly1305, under a data key. The data key is wrapped for each reader with that reader's public key,
   so the contract never holds a key.
2. **Store ciphertext and a version.** Add `set_encrypted_metadata(creator, invoice_id, ciphertext, key_version)`
   (creator authorisation, invoice not yet released), which stores the ciphertext in persistent storage under the
   invoice and keeps `sha256(ciphertext)` next to it. `get_encrypted_metadata(invoice_id)` returns both. Keep a size
   limit on the ciphertext.
3. **Rotation.** Rotation is done off-chain: decrypt with the old key, encrypt with the new one, and call
   `set_encrypted_metadata` again with a higher `key_version`. The contract only checks that the version does not go
   down, and records the old hash.
4. **Audit trail.** Append `(action, caller, key_version, ciphertext_hash, ledger)` to a per-invoice list, or emit
   an event for each write, which an indexer can read. Reads are not visible on-chain, so reads cannot be audited
   there. A read audit has to be kept by whatever off-chain service hands out the wrapped keys.
5. **Remove or fix the stub API.** Either delete `encrypt_data`, `decrypt_data` and the key functions, or make
   them fail loudly until they are real. The current behaviour is worse than not having them.

## Open questions for the maintainers

- Who may read: the creator, recipients, payers, an auditor?
- Is metadata allowed to change after the first payment, and should a change be shown to payers?
- Should the existing stub functions be removed, which changes the public interface, or kept?

## What this change does

Records the status and a design. It changes no code, adds no tests, and was not built or run.
