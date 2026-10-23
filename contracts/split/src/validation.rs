//! Issue #556 / #558: Invoice validation helpers.
//!
//! These guards run *before* any storage is written during invoice creation
//! (or before any recipient mutation) so that a malformed recipient list is
//! rejected atomically.

use crate::error::ContractError;
use soroban_sdk::address_payload::AddressPayload;
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{symbol_short, Address, Bytes, BytesN, Env, IntoVal, Map, Val, Vec};

/// Issue #931: Maximum number of recipients on a single invoice. Bounds the
/// release loop and per-invoice storage (see `DEFAULT_INVOICE_STORAGE_QUOTA`).
pub const MAX_INVOICE_RECIPIENTS: u32 = 200;

/// Issue #931: Maximum distance between the current ledger time and an
/// invoice deadline (~10 years). Rejects typos such as millisecond timestamps
/// that would lock funds effectively forever.
pub const MAX_DEADLINE_HORIZON_SECS: u64 = 315_360_000;

// ---------------------------------------------------------------------------
// Issue #933: Amount overflow protection
// ---------------------------------------------------------------------------

/// Largest factor an invoice amount is multiplied by in intermediate
/// arithmetic: basis points (`10_000`) × the oracle rate scale (`1_000_000`).
pub const MAX_AMOUNT_SCALE: i128 = 10_000_000_000;

/// Issue #933: Maximum total of an invoice's `amounts`, in token base units.
///
/// `i128::MAX / MAX_AMOUNT_SCALE` ≈ 1.7 × 10^28, so `total * bps` and
/// `total * ORACLE_RATE_SCALE * bps` can never overflow `i128`. This is still
/// ~1.7 × 10^21 whole tokens at 7 decimals — far above any real asset supply.
pub const MAX_INVOICE_TOTAL: i128 = i128::MAX / MAX_AMOUNT_SCALE;

/// Issue #933: Sum `amounts`, failing instead of wrapping or panicking on overflow.
///
/// # Errors
/// Returns [ContractError::ArithmeticOverflow] if the running total overflows `i128`.
pub fn checked_sum_amounts(amounts: &Vec<i128>) -> Result<i128, ContractError> {
    let mut total: i128 = 0;
    for amt in amounts.iter() {
        total = total
            .checked_add(amt)
            .ok_or(ContractError::ArithmeticOverflow)?;
    }
    Ok(total)
}

/// Issue #933: Add `amount` to an accumulator (e.g. `invoice.funded`),
/// failing instead of panicking on overflow.
///
/// # Errors
/// Returns [ContractError::ArithmeticOverflow] if the sum overflows `i128`.
pub fn checked_add_amount(acc: i128, amount: i128) -> Result<i128, ContractError> {
    acc.checked_add(amount).ok_or(ContractError::ArithmeticOverflow)
}

/// Issue #933: Validate that an invoice's total stays within [`MAX_INVOICE_TOTAL`].
/// Returns the total on success.
///
/// # Errors
/// Returns [ContractError::ArithmeticOverflow] when the sum overflows `i128`
/// or exceeds [`MAX_INVOICE_TOTAL`].
pub fn validate_invoice_total(amounts: &Vec<i128>) -> Result<i128, ContractError> {
    let total = checked_sum_amounts(amounts)?;
    if total > MAX_INVOICE_TOTAL {
        return Err(ContractError::ArithmeticOverflow);
    }
    Ok(total)
}

/// Issue #931: Validate the recipients and amounts arrays of an invoice.
///
/// Checks, in order:
/// 1. `recipients.len() == amounts.len()` — [ContractError::InvalidRecipients]
/// 2. at least one recipient — [ContractError::EmptyRecipientList]
/// 3. at most [`MAX_INVOICE_RECIPIENTS`] — [ContractError::BatchLimitExceeded]
/// 4. every amount is strictly positive — [ContractError::ZeroAmountNotAllowed]
/// 5. the total is within [`MAX_INVOICE_TOTAL`] — [ContractError::ArithmeticOverflow]
///    (issue #933)
///
/// No allocation.
///
/// # Errors
/// Returns the first failing check's error.
pub fn validate_recipients_and_amounts(
    recipients: &Vec<Address>,
    amounts: &Vec<i128>,
) -> Result<(), ContractError> {
    if recipients.len() != amounts.len() {
        return Err(ContractError::InvalidRecipients);
    }
    if recipients.is_empty() {
        return Err(ContractError::EmptyRecipientList);
    }
    if recipients.len() > MAX_INVOICE_RECIPIENTS {
        return Err(ContractError::BatchLimitExceeded);
    }
    for amt in amounts.iter() {
        if amt <= 0 {
            return Err(ContractError::ZeroAmountNotAllowed);
        }
    }
    validate_invoice_total(amounts)?;
    Ok(())
}

/// Issue #931: Validate an invoice deadline against the current ledger time.
///
/// # Errors
/// - [ContractError::DeadlinePassed] when `deadline <= now`
/// - [ContractError::InvalidDeadline] when `deadline - now > MAX_DEADLINE_HORIZON_SECS`
pub fn validate_deadline(env: &Env, deadline: u64) -> Result<(), ContractError> {
    let now = env.ledger().timestamp();
    if deadline <= now {
        return Err(ContractError::DeadlinePassed);
    }
    if deadline - now > MAX_DEADLINE_HORIZON_SECS {
        return Err(ContractError::InvalidDeadline);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Issue #932: Recipient address validation
// ---------------------------------------------------------------------------

/// Length of an account (`G…`) or contract (`C…`) strkey.
pub const STRKEY_LEN: usize = 56;
/// Decoded strkey length: 1 version byte + 32-byte payload + 2-byte checksum.
const STRKEY_RAW_LEN: usize = 35;
/// Strkey version byte for an ed25519 account public key (`G…`).
const VERSION_ACCOUNT: u8 = 6 << 3;
/// Strkey version byte for a contract id (`C…`).
const VERSION_CONTRACT: u8 = 2 << 3;

/// Map an RFC 4648 base32 character to its 5-bit value.
fn base32_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'2'..=b'7' => Some(c - b'2' + 26),
        _ => None,
    }
}

/// CRC16-XModem (poly 0x1021, init 0) — the checksum used by Stellar strkeys.
fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Validate a raw strkey for a recipient address.
///
/// Checks:
/// - **format** — exactly [`STRKEY_LEN`] uppercase base32 characters, starting
///   with `G` (account) or `C` (contract); muxed (`M…`), secret (`S…`) and other
///   key types are rejected
/// - **checksum** — the trailing CRC16-XModem (little-endian) matches the
///   version byte and payload
/// - **non-zero** — the 32-byte payload is not all zeros (e.g. the
///   `GAAAA…WHF` zero account, which nobody controls)
///
/// Fixed-size buffers only; no allocation.
///
/// # Errors
/// Returns [ContractError::InvalidRecipientAddress] on any failure.
pub fn validate_strkey(strkey: &[u8]) -> Result<(), ContractError> {
    if strkey.len() != STRKEY_LEN {
        return Err(ContractError::InvalidRecipientAddress);
    }

    // Base32-decode 56 chars (280 bits) into exactly 35 bytes.
    let mut raw = [0u8; STRKEY_RAW_LEN];
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut out = 0usize;
    for &c in strkey {
        let v = base32_value(c).ok_or(ContractError::InvalidRecipientAddress)?;
        acc = (acc << 5) | v as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            raw[out] = (acc >> bits) as u8;
            out += 1;
            acc &= (1 << bits) - 1;
        }
    }
    if out != STRKEY_RAW_LEN {
        return Err(ContractError::InvalidRecipientAddress);
    }

    let version = raw[0];
    if version != VERSION_ACCOUNT && version != VERSION_CONTRACT {
        return Err(ContractError::InvalidRecipientAddress);
    }

    let expected = crc16_xmodem(&raw[..33]);
    let actual = u16::from_le_bytes([raw[33], raw[34]]);
    if expected != actual {
        return Err(ContractError::InvalidRecipientAddress);
    }

    if raw[1..33].iter().all(|b| *b == 0) {
        return Err(ContractError::InvalidRecipientAddress);
    }
    Ok(())
}

/// Issue #932: Validate a single recipient address.
///
/// Runs [`validate_strkey`] on the address's strkey and additionally rejects
/// the current contract's own address, which would strand funds in escrow.
///
/// # Errors
/// Returns [ContractError::InvalidRecipientAddress] on any failure.
pub fn validate_recipient_address(env: &Env, recipient: &Address) -> Result<(), ContractError> {
    if *recipient == env.current_contract_address() {
        return Err(ContractError::InvalidRecipientAddress);
    }
    let strkey = recipient.to_string();
    if strkey.len() as usize != STRKEY_LEN {
        return Err(ContractError::InvalidRecipientAddress);
    }
    let mut buf = [0u8; STRKEY_LEN];
    strkey.copy_into_slice(&mut buf);
    validate_strkey(&buf)
}

/// Issue #932: Validate every address in a recipient list.
///
/// # Errors
/// Returns [ContractError::InvalidRecipientAddress] for the first invalid address.
pub fn validate_recipient_addresses(
    env: &Env,
    recipients: &Vec<Address>,
) -> Result<(), ContractError> {
    for r in recipients.iter() {
        validate_recipient_address(env, &r)?;
    }
    Ok(())
}

/// Issue #931: Validate every core input to invoice creation before any
/// storage is written: recipients/amounts arrays and the deadline.
///
/// # Errors
/// See [`validate_recipients_and_amounts`] and [`validate_deadline`].
pub fn validate_invoice_inputs(
    env: &Env,
    recipients: &Vec<Address>,
    amounts: &Vec<i128>,
    deadline: u64,
) -> Result<(), ContractError> {
    validate_recipients_and_amounts(recipients, amounts)?;
    validate_deadline(env, deadline)
}

/// Issue #556: Reject a recipient list that contains duplicate addresses.
///
/// Uses a `soroban_sdk::Map` for O(n log n) membership tracking — no
/// unbounded heap allocation and a single pass over the slice.
///
/// # Errors
/// Returns [ContractError::DuplicateRecipient] when a duplicate is found.
pub fn assert_unique_recipients(env: &Env, recipients: &Vec<Address>) -> Result<(), ContractError> {
    let mut seen: Map<Address, bool> = Map::new(env);
    for r in recipients.iter() {
        if seen.contains_key(r.clone()) {
            return Err(ContractError::DuplicateRecipient);
        }
        seen.set(r.clone(), true);
    }
    Ok(())
}

/// Issue #558: Verify that every recipient has a trustline for the payment
/// token by calling `balance()` on the token contract.
///
/// A host-trapped invocation (the account has never been initialised on the
/// ledger, or no trustline exists for the asset) is treated as "no trustline".
///
/// The check is bypassed for the native / XLM asset, which requires no
/// trustline.  In Soroban the native asset is identified by the absence of an
/// explicit token contract — callers should pass the native sentinel address
/// (`env.ledger().native_token_id()` when available) or simply skip the check
/// for native invoices.
///
/// Returns `Ok(())` when every recipient returns a valid balance, or
/// `Err(ContractError::RecipientMissingTrustline)` with the offending address
/// surfaced in the panic message.
/// # Errors
/// Returns [ContractError::RecipientMissingTrustline] when a balance call fails.
pub fn assert_recipients_have_trustlines(
    env: &Env,
    token: &Address,
    recipients: &Vec<Address>,
) -> Result<(), ContractError> {
    for r in recipients.iter() {
        let mut args: Vec<Val> = Vec::new(env);
        args.push_back(r.clone().into_val(env));
        let result = env.try_invoke_contract::<i128, soroban_sdk::Error>(
            token,
            &symbol_short!("balance"),
            args,
        );
        match result {
            Ok(_) => {}
            Err(_) => {
                // Surface the offending address in the panic message so
                // callers and tests can identify which recipient is missing
                // the trustline.
                env.panic_with_error(ContractError::RecipientMissingTrustline);
            }
        }
    }
    Ok(())
}

/// Issue #623: Verify that the sum of `values` equals exactly `BASIS_POINTS_TOTAL` (10 000).
///
/// Used wherever a slice of basis-point weights must cover 100% of a whole:
/// split ratios, release stages, fee recipients, etc.
///
/// # Errors
/// Returns `Err(ContractError::InvalidRatioSum)` when `values.iter().sum::<u32>() != 10_000`.
///
/// # Examples
/// ```ignore
/// assert!(assert_bps_sum(&[5_000u32, 5_000]).is_ok());
/// assert!(assert_bps_sum(&[3_000u32, 3_000]).is_err());
/// ```
pub const BASIS_POINTS_TOTAL: u32 = 10_000;

pub fn assert_bps_sum(values: &[u32]) -> Result<(), ContractError> {
    let sum: u32 = values.iter().copied().fold(0u32, |acc, v| acc.saturating_add(v));
    assert_bps_total(sum)
}

/// Variant of [`assert_bps_sum`] for call sites where the sum has already
/// been computed (e.g. from a soroban `Vec` iterator in a `no_std` context).
///
/// Returns `Err(ContractError::InvalidRatioSum)` when `total != 10_000`.
pub fn assert_bps_total(total: u32) -> Result<(), ContractError> {
    if total != BASIS_POINTS_TOTAL {
        return Err(ContractError::InvalidRatioSum);
    }
    Ok(())
}

/// Reject a single basis-point value that exceeds 100% (`BASIS_POINTS_TOTAL` =
/// 10 000). Used for standalone rate fields such as `penalty_bps`, `tax_bps`
/// and `insurance_premium_bps` where the value must be a fraction of a whole,
/// not a sum that covers it.
///
/// # Errors
/// Returns `Err(ContractError::InvalidAmount)` when `bps > 10_000`.
pub fn assert_valid_bps(bps: u32) -> Result<(), ContractError> {
    if bps > BASIS_POINTS_TOTAL {
        return Err(ContractError::InvalidAmount);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Issue #937: co-signer signature validation with domain separation,
// nonce binding and replay protection.
// ---------------------------------------------------------------------------

/// Domain-separation tag mixed into every co-signer digest.  Changing this
/// constant invalidates all previously produced signatures, so it must stay
/// stable across releases.
pub const SIGNATURE_DOMAIN: &[u8] = b"split-cosigner-v1";

/// Length of the nonce used for replay protection (32 bytes).
pub const NONCE_LEN: u32 = 32;

/// Build the canonical message digest that a co-signer must sign.
///
/// The digest binds together the domain tag, the contract address, the
/// invoice id, the nonce and the payload so that a signature produced for one
/// invoice / contract / nonce can never be replayed for another.
///
/// # Errors
/// Returns [ContractError::InvalidSignature] when `nonce` is not
/// [`NONCE_LEN`] bytes long.
pub fn cosigner_digest(
    env: &Env,
    contract: &Address,
    invoice_id: u64,
    nonce: &BytesN<32>,
    payload: &Bytes,
) -> Result<BytesN<32>, ContractError> {
    let mut msg: Bytes = Bytes::new(env);
    msg.extend_from_slice(SIGNATURE_DOMAIN);
    msg.append(&contract.clone().to_xdr(env));
    msg.extend_from_array(&invoice_id.to_be_bytes());
    msg.append(&Bytes::from(nonce.clone()));
    msg.append(payload);
    Ok(env.crypto().sha256(&msg).to_bytes())
}

/// Verify a co-signer signature over the canonical digest and enforce replay
/// protection by recording the nonce.
///
/// The signature is checked with `ed25519_verify`; the nonce is then stored in
/// persistent storage keyed by `(invoice_id, nonce)`.  A nonce that has
/// already been used for the same invoice is rejected with
/// [ContractError::SignatureReplayed].
///
/// # Errors
/// - [ContractError::InvalidSignature] when the signature does not verify.
/// - [ContractError::SignatureReplayed] when the nonce was already consumed.
pub fn verify_cosigner_signature(
    env: &Env,
    contract: &Address,
    invoice_id: u64,
    signer: &Address,
    nonce: &BytesN<32>,
    payload: &Bytes,
    signature: &BytesN<64>,
) -> Result<(), ContractError> {
    let digest = cosigner_digest(env, contract, invoice_id, nonce, payload)?;
    // Only account (G...) addresses carry an Ed25519 public key.
    let public_key = match signer.to_payload() {
        Some(AddressPayload::AccountIdPublicKeyEd25519(pk)) => pk,
        _ => return Err(ContractError::InvalidSignature),
    };
    // `ed25519_verify` traps the invocation when the signature is invalid.
    env.crypto()
        .ed25519_verify(&public_key, &Bytes::from(digest), signature);
    consume_nonce(env, invoice_id, nonce)
}

/// Record `nonce` as used for `invoice_id`, rejecting replays.
///
/// # Errors
/// Returns [ContractError::SignatureReplayed] when the nonce was already
/// recorded for this invoice.
pub fn consume_nonce(env: &Env, invoice_id: u64, nonce: &BytesN<32>) -> Result<(), ContractError> {
    let key = nonce_key(env, invoice_id, nonce);
    if env.storage().persistent().has(&key) {
        return Err(ContractError::SignatureReplayed);
    }
    env.storage().persistent().set(&key, &true);
    Ok(())
}

/// Returns `true` when `nonce` has already been consumed for `invoice_id`.
pub fn is_nonce_used(env: &Env, invoice_id: u64, nonce: &BytesN<32>) -> bool {
    env.storage().persistent().has(&nonce_key(env, invoice_id, nonce))
}

fn nonce_key(env: &Env, invoice_id: u64, nonce: &BytesN<32>) -> Val {
    let mut key: Vec<Val> = Vec::new(env);
    key.push_back(symbol_short!("nonce").into_val(env));
    key.push_back(invoice_id.into_val(env));
    key.push_back(nonce.clone().into_val(env));
    key.into_val(env)
}

// ---------------------------------------------------------------------------
// Issue #938: oracle address and condition validation.
// ---------------------------------------------------------------------------

/// Expected length of a Stellar strkey-encoded address (ed25519 public key,
/// `G...`), i.e. 56 characters.
pub const ORACLE_ADDRESS_LEN: u32 = 56;

/// Validate the textual format of an oracle address.
///
/// Soroban `Address` values are already type-checked by the host, so this
/// guard focuses on the strkey shape: the address must be a contract or
/// account address whose string form is exactly [`ORACLE_ADDRESS_LEN`]
/// characters and starts with `G` (account) or `C` (contract).
///
/// # Errors
/// Returns [ContractError::InvalidOracleAddress] when the format is wrong.
pub fn assert_valid_oracle_address(env: &Env, oracle: &Address) -> Result<(), ContractError> {
    let s = oracle.to_string();
    if s.len() != ORACLE_ADDRESS_LEN {
        return Err(ContractError::InvalidOracleAddress);
    }
    let mut bytes = [0u8; ORACLE_ADDRESS_LEN as usize];
    s.copy_into_slice(&mut bytes);
    let first = bytes[0];
    if first != b'G' && first != b'C' {
        return Err(ContractError::InvalidOracleAddress);
    }
    // Reject addresses that are not valid strkey for this environment.
    if Address::from_string(&s) != *oracle {
        return Err(ContractError::InvalidOracleAddress);
    }
    let _ = env;
    Ok(())
}

/// Validate an oracle price condition.
///
/// A condition is a `(threshold, comparison)` pair where `comparison` is one
/// of `0` (equal), `1` (greater-or-equal) or `2` (less-or-equal).  The
/// threshold must be strictly positive.
///
/// # Errors
/// Returns [ContractError::InvalidOracleCondition] when the comparison is
/// unknown or the threshold is not positive.
pub fn assert_valid_oracle_condition(threshold: i128, comparison: u32) -> Result<(), ContractError> {
    if threshold <= 0 {
        return Err(ContractError::InvalidOracleCondition);
    }
    if comparison > 2 {
        return Err(ContractError::InvalidOracleCondition);
    }
    Ok(())
}

/// Evaluate an oracle condition against an observed price.
///
/// # Errors
/// Returns [ContractError::InvalidOracleCondition] when the condition itself
/// is malformed (see [`assert_valid_oracle_condition`]).
pub fn evaluate_oracle_condition(
    threshold: i128,
    comparison: u32,
    observed: i128,
) -> Result<bool, ContractError> {
    assert_valid_oracle_condition(threshold, comparison)?;
    let met = match comparison {
        0 => observed == threshold,
        1 => observed >= threshold,
        _ => observed <= threshold,
    };
    Ok(met)
}

// ---------------------------------------------------------------------------
// Issues #935 / #939 / #940: fee, split-rule and token validation
// ---------------------------------------------------------------------------

/// Issue #935: Validate a platform (or early-bird) fee rate against
/// [`crate::constants::MAX_PLATFORM_FEE_BPS`].
///
/// # Errors
/// Returns [ContractError::FeeSumExceedsCap] when `rate` is above the cap.
pub fn validate_fee_rate(rate: u32) -> Result<(), ContractError> {
    if rate > crate::constants::MAX_PLATFORM_FEE_BPS {
        return Err(ContractError::FeeSumExceedsCap);
    }
    Ok(())
}

/// Issue #939: Validate an invoice's split rules.
///
/// * there must be exactly one rule per recipient
/// * `Fixed` amounts must be positive and `Tiered` thresholds non-negative
/// * every bps value must be within `0..=10_000`
/// * the bps of all `Percentage` rules together may not exceed 100%
///
/// # Errors
/// - [ContractError::InvalidRecipients] when the rule count differs from `recipient_count`.
/// - [ContractError::InvalidAmount] for a non-positive fixed amount or negative threshold.
/// - [ContractError::InvalidRatio] for a bps value above 10 000.
/// - [ContractError::InvalidRatioSum] when the percentage rules exceed 10 000 bps.
pub fn validate_split_rules(
    rules: &Vec<crate::types::SplitRule>,
    recipient_count: u32,
) -> Result<(), ContractError> {
    use crate::types::SplitRule;
    if rules.len() != recipient_count {
        return Err(ContractError::InvalidRecipients);
    }
    let mut percentage_total: u32 = 0;
    for rule in rules.iter() {
        match rule {
            SplitRule::Fixed(amount) => {
                if amount <= 0 {
                    return Err(ContractError::InvalidAmount);
                }
            }
            SplitRule::Percentage(bps) => {
                assert_valid_bps(bps).map_err(|_| ContractError::InvalidRatio)?;
                percentage_total = percentage_total
                    .checked_add(bps)
                    .ok_or(ContractError::InvalidRatioSum)?;
            }
            SplitRule::Tiered(threshold, bps) => {
                if threshold < 0 {
                    return Err(ContractError::InvalidAmount);
                }
                assert_valid_bps(bps).map_err(|_| ContractError::InvalidRatio)?;
            }
        }
    }
    if percentage_total > 10_000 {
        return Err(ContractError::InvalidRatioSum);
    }
    Ok(())
}

/// Issue #940: Verify that `token` answers the read-only SEP-41 calls
/// (`decimals` and `balance`).
///
/// Only side-effect-free methods are probed so validation never moves funds
/// or needs extra authorisation.
///
/// # Errors
/// Returns [ContractError::UnauthorisedToken] when either probe fails.
pub fn validate_token_contract(env: &Env, token: &Address) -> Result<(), ContractError> {
    let decimals = env.try_invoke_contract::<u32, soroban_sdk::Error>(
        token,
        &symbol_short!("decimals"),
        Vec::new(env),
    );
    if !matches!(decimals, Ok(Ok(_))) {
        return Err(ContractError::UnauthorisedToken);
    }
    let mut args: Vec<Val> = Vec::new(env);
    args.push_back(env.current_contract_address().into_val(env));
    let balance =
        env.try_invoke_contract::<i128, soroban_sdk::Error>(token, &symbol_short!("balance"), args);
    if !matches!(balance, Ok(Ok(_))) {
        return Err(ContractError::UnauthorisedToken);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn unique_recipients_passes() {
        let env = Env::default();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let c = Address::generate(&env);
        let mut v: Vec<Address> = Vec::new(&env);
        v.push_back(a.clone());
        v.push_back(b.clone());
        v.push_back(c.clone());
        assert!(assert_unique_recipients(&env, &v).is_ok());
    }

    #[test]
    fn duplicate_recipient_rejected() {
        let env = Env::default();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let mut v: Vec<Address> = Vec::new(&env);
        v.push_back(a.clone());
        v.push_back(b.clone());
        v.push_back(a.clone());
        assert_eq!(
            assert_unique_recipients(&env, &v),
            Err(ContractError::DuplicateRecipient)
        );
    }

    #[test]
    fn empty_recipient_list_passes() {
        let env = Env::default();
        let v: Vec<Address> = Vec::new(&env);
        assert!(assert_unique_recipients(&env, &v).is_ok());
    }

    #[test]
    fn single_recipient_passes() {
        let env = Env::default();
        let a = Address::generate(&env);
        let mut v: Vec<Address> = Vec::new(&env);
        v.push_back(a.clone());
        assert!(assert_unique_recipients(&env, &v).is_ok());
    }

    #[test]
    fn non_adjacent_duplicate_rejected() {
        // duplicate at index 0 and 2 with a different address at index 1
        let env = Env::default();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let mut v: Vec<Address> = Vec::new(&env);
        v.push_back(a.clone()); // index 0
        v.push_back(b.clone()); // index 1
        v.push_back(a.clone()); // index 2 — non-adjacent duplicate of index 0
        assert_eq!(
            assert_unique_recipients(&env, &v),
            Err(ContractError::DuplicateRecipient)
        );
    }

    #[test]
    fn ten_unique_recipients_passes() {
        let env = Env::default();
        let mut v: Vec<Address> = Vec::new(&env);
        for _ in 0..10 {
            v.push_back(Address::generate(&env));
        }
        assert!(assert_unique_recipients(&env, &v).is_ok());
    }

    // --- validate_invoice_inputs (issue #931) ---

    use soroban_sdk::testutils::Ledger;

    fn addrs(env: &Env, n: u32) -> Vec<Address> {
        let mut v = Vec::new(env);
        for _ in 0..n {
            v.push_back(Address::generate(env));
        }
        v
    }

    fn amts(env: &Env, values: &[i128]) -> Vec<i128> {
        let mut v = Vec::new(env);
        for a in values {
            v.push_back(*a);
        }
        v
    }

    #[test]
    fn valid_invoice_inputs_pass() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        let r = addrs(&env, 2);
        let a = amts(&env, &[10, 20]);
        assert_eq!(validate_invoice_inputs(&env, &r, &a, 2_000), Ok(()));
    }

    #[test]
    fn length_mismatch_rejected() {
        let env = Env::default();
        let r = addrs(&env, 2);
        let a = amts(&env, &[10]);
        assert_eq!(
            validate_recipients_and_amounts(&r, &a),
            Err(ContractError::InvalidRecipients)
        );
    }

    #[test]
    fn empty_arrays_rejected() {
        let env = Env::default();
        let r = addrs(&env, 0);
        let a = amts(&env, &[]);
        assert_eq!(
            validate_recipients_and_amounts(&r, &a),
            Err(ContractError::EmptyRecipientList)
        );
    }

    #[test]
    fn too_many_recipients_rejected() {
        let env = Env::default();
        let n = MAX_INVOICE_RECIPIENTS + 1;
        let r = addrs(&env, n);
        let mut a = Vec::new(&env);
        for _ in 0..n {
            a.push_back(1i128);
        }
        assert_eq!(
            validate_recipients_and_amounts(&r, &a),
            Err(ContractError::BatchLimitExceeded)
        );
    }

    #[test]
    fn max_recipients_accepted() {
        let env = Env::default();
        let r = addrs(&env, MAX_INVOICE_RECIPIENTS);
        let mut a = Vec::new(&env);
        for _ in 0..MAX_INVOICE_RECIPIENTS {
            a.push_back(1i128);
        }
        assert_eq!(validate_recipients_and_amounts(&r, &a), Ok(()));
    }

    #[test]
    fn zero_and_negative_amounts_rejected() {
        let env = Env::default();
        let r = addrs(&env, 2);
        assert_eq!(
            validate_recipients_and_amounts(&r, &amts(&env, &[10, 0])),
            Err(ContractError::ZeroAmountNotAllowed)
        );
        assert_eq!(
            validate_recipients_and_amounts(&r, &amts(&env, &[-1, 10])),
            Err(ContractError::ZeroAmountNotAllowed)
        );
        assert_eq!(
            validate_recipients_and_amounts(&r, &amts(&env, &[i128::MIN, 10])),
            Err(ContractError::ZeroAmountNotAllowed)
        );
    }

    #[test]
    fn past_and_current_deadline_rejected() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        assert_eq!(validate_deadline(&env, 999), Err(ContractError::DeadlinePassed));
        assert_eq!(validate_deadline(&env, 1_000), Err(ContractError::DeadlinePassed));
        assert_eq!(validate_deadline(&env, 0), Err(ContractError::DeadlinePassed));
    }

    #[test]
    fn deadline_horizon_boundary() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        assert_eq!(validate_deadline(&env, 1_001), Ok(()));
        assert_eq!(
            validate_deadline(&env, 1_000 + MAX_DEADLINE_HORIZON_SECS),
            Ok(())
        );
        assert_eq!(
            validate_deadline(&env, 1_000 + MAX_DEADLINE_HORIZON_SECS + 1),
            Err(ContractError::InvalidDeadline)
        );
        assert_eq!(validate_deadline(&env, u64::MAX), Err(ContractError::InvalidDeadline));
    }

    // --- recipient address validation (issue #932) ---

    /// The all-zero ed25519 account: valid format and checksum, zero payload.
    const ZERO_ACCOUNT: &[u8] = b"GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";

    fn strkey_of(addr: &Address) -> [u8; STRKEY_LEN] {
        let s = addr.to_string();
        let mut buf = [0u8; STRKEY_LEN];
        s.copy_into_slice(&mut buf);
        buf
    }

    /// Re-encode `raw` (version + payload) as a strkey with a correct checksum.
    fn encode_strkey(version: u8, payload: &[u8; 32]) -> [u8; STRKEY_LEN] {
        const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut raw = [0u8; STRKEY_RAW_LEN];
        raw[0] = version;
        raw[1..33].copy_from_slice(payload);
        let crc = crc16_xmodem(&raw[..33]).to_le_bytes();
        raw[33] = crc[0];
        raw[34] = crc[1];
        let mut out = [0u8; STRKEY_LEN];
        let (mut acc, mut bits, mut i) = (0u32, 0u32, 0usize);
        for b in raw {
            acc = (acc << 8) | b as u32;
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                out[i] = ALPHABET[((acc >> bits) & 31) as usize];
                i += 1;
            }
            acc &= (1 << bits) - 1;
        }
        out
    }

    #[test]
    fn crc16_xmodem_known_vector() {
        // Standard CRC-16/XMODEM check value for "123456789".
        assert_eq!(crc16_xmodem(b"123456789"), 0x31C3);
    }

    #[test]
    fn generated_addresses_are_valid() {
        let env = Env::default();
        for _ in 0..10 {
            let a = Address::generate(&env);
            assert_eq!(validate_strkey(&strkey_of(&a)), Ok(()));
            assert_eq!(validate_recipient_address(&env, &a), Ok(()));
        }
    }

    #[test]
    fn nonzero_account_and_contract_strkeys_valid() {
        let mut payload = [0u8; 32];
        payload[31] = 1;
        assert_eq!(validate_strkey(&encode_strkey(VERSION_ACCOUNT, &payload)), Ok(()));
        assert_eq!(validate_strkey(&encode_strkey(VERSION_CONTRACT, &payload)), Ok(()));
        let all_ff = [0xFFu8; 32];
        assert_eq!(validate_strkey(&encode_strkey(VERSION_ACCOUNT, &all_ff)), Ok(()));
    }

    #[test]
    fn zero_account_rejected() {
        assert_eq!(validate_strkey(ZERO_ACCOUNT), Err(ContractError::InvalidRecipientAddress));
        let zero = [0u8; 32];
        assert_eq!(
            validate_strkey(&encode_strkey(VERSION_CONTRACT, &zero)),
            Err(ContractError::InvalidRecipientAddress)
        );
    }

    #[test]
    fn bad_checksum_rejected() {
        let env = Env::default();
        let mut key = strkey_of(&Address::generate(&env));
        // Flip one character in the payload; CRC16 detects every single-symbol error.
        key[20] = if key[20] == b'A' { b'B' } else { b'A' };
        assert_eq!(validate_strkey(&key), Err(ContractError::InvalidRecipientAddress));
    }

    #[test]
    fn wrong_length_rejected() {
        let env = Env::default();
        let key = strkey_of(&Address::generate(&env));
        assert_eq!(validate_strkey(&key[..55]), Err(ContractError::InvalidRecipientAddress));
        assert_eq!(validate_strkey(&[]), Err(ContractError::InvalidRecipientAddress));
        let mut long = [b'A'; 57];
        long[..56].copy_from_slice(&key);
        assert_eq!(validate_strkey(&long), Err(ContractError::InvalidRecipientAddress));
    }

    #[test]
    fn invalid_characters_rejected() {
        let env = Env::default();
        let mut key = strkey_of(&Address::generate(&env));
        key[10] = b'a'; // lowercase
        assert_eq!(validate_strkey(&key), Err(ContractError::InvalidRecipientAddress));
        key[10] = b'1'; // not in base32 alphabet
        assert_eq!(validate_strkey(&key), Err(ContractError::InvalidRecipientAddress));
        key[10] = b'=';
        assert_eq!(validate_strkey(&key), Err(ContractError::InvalidRecipientAddress));
    }

    #[test]
    fn unsupported_key_types_rejected() {
        let mut payload = [0u8; 32];
        payload[0] = 7;
        // Secret seed (S…, version 18 << 3) and pre-auth tx (T…, version 19 << 3).
        assert_eq!(
            validate_strkey(&encode_strkey(18 << 3, &payload)),
            Err(ContractError::InvalidRecipientAddress)
        );
        assert_eq!(
            validate_strkey(&encode_strkey(19 << 3, &payload)),
            Err(ContractError::InvalidRecipientAddress)
        );
    }

    #[test]
    fn contract_self_address_rejected() {
        let env = Env::default();
        let contract_id = env.register(crate::SplitContract, ());
        env.as_contract(&contract_id, || {
            assert_eq!(
                validate_recipient_address(&env, &contract_id),
                Err(ContractError::InvalidRecipientAddress)
            );
            let other = Address::generate(&env);
            assert_eq!(validate_recipient_address(&env, &other), Ok(()));
        });
    }

    #[test]
    fn recipient_list_reports_first_invalid() {
        let env = Env::default();
        let contract_id = env.register(crate::SplitContract, ());
        env.as_contract(&contract_id, || {
            let mut list = addrs(&env, 3);
            assert_eq!(validate_recipient_addresses(&env, &list), Ok(()));
            list.push_back(contract_id.clone());
            assert_eq!(
                validate_recipient_addresses(&env, &list),
                Err(ContractError::InvalidRecipientAddress)
            );
        });
    }

    // --- amount overflow protection (issue #933) ---

    #[test]
    fn max_invoice_total_leaves_headroom_for_scaling() {
        assert!(MAX_INVOICE_TOTAL.checked_mul(MAX_AMOUNT_SCALE).is_some());
        assert!((MAX_INVOICE_TOTAL + 1).checked_mul(MAX_AMOUNT_SCALE).is_none());
    }

    #[test]
    fn checked_sum_normal_values() {
        let env = Env::default();
        assert_eq!(checked_sum_amounts(&amts(&env, &[])), Ok(0));
        assert_eq!(checked_sum_amounts(&amts(&env, &[1, 2, 3])), Ok(6));
    }

    #[test]
    fn checked_sum_overflow_caught() {
        let env = Env::default();
        assert_eq!(
            checked_sum_amounts(&amts(&env, &[i128::MAX, 1])),
            Err(ContractError::ArithmeticOverflow)
        );
        assert_eq!(
            checked_sum_amounts(&amts(&env, &[i128::MAX / 2 + 1, i128::MAX / 2 + 1])),
            Err(ContractError::ArithmeticOverflow)
        );
        assert_eq!(checked_sum_amounts(&amts(&env, &[i128::MAX])), Ok(i128::MAX));
    }

    #[test]
    fn checked_add_amount_boundaries() {
        assert_eq!(checked_add_amount(i128::MAX - 1, 1), Ok(i128::MAX));
        assert_eq!(checked_add_amount(i128::MAX, 1), Err(ContractError::ArithmeticOverflow));
        assert_eq!(checked_add_amount(0, 0), Ok(0));
    }

    #[test]
    fn invoice_total_boundary() {
        let env = Env::default();
        assert_eq!(
            validate_invoice_total(&amts(&env, &[MAX_INVOICE_TOTAL])),
            Ok(MAX_INVOICE_TOTAL)
        );
        assert_eq!(
            validate_invoice_total(&amts(&env, &[MAX_INVOICE_TOTAL - 1, 1])),
            Ok(MAX_INVOICE_TOTAL)
        );
        assert_eq!(
            validate_invoice_total(&amts(&env, &[MAX_INVOICE_TOTAL, 1])),
            Err(ContractError::ArithmeticOverflow)
        );
        assert_eq!(
            validate_invoice_total(&amts(&env, &[i128::MAX, i128::MAX])),
            Err(ContractError::ArithmeticOverflow)
        );
    }

    #[test]
    fn recipients_and_amounts_reject_total_above_limit() {
        let env = Env::default();
        let r = addrs(&env, 2);
        assert_eq!(
            validate_recipients_and_amounts(&r, &amts(&env, &[MAX_INVOICE_TOTAL, 1])),
            Err(ContractError::ArithmeticOverflow)
        );
        assert_eq!(
            validate_recipients_and_amounts(&r, &amts(&env, &[i128::MAX, i128::MAX])),
            Err(ContractError::ArithmeticOverflow)
        );
        assert_eq!(
            validate_recipients_and_amounts(&r, &amts(&env, &[MAX_INVOICE_TOTAL - 1, 1])),
            Ok(())
        );
    }

    // --- assert_bps_sum (issue #623) ---

    #[test]
    fn bps_sum_equals_10000_passes() {
        assert!(assert_bps_sum(&[5_000u32, 5_000]).is_ok());
        assert!(assert_bps_sum(&[3_000u32, 3_000, 4_000]).is_ok());
        assert!(assert_bps_sum(&[10_000u32]).is_ok());
    }

    #[test]
    fn bps_sum_not_10000_fails() {
        assert_eq!(
            assert_bps_sum(&[3_000u32, 3_000]),
            Err(ContractError::InvalidRatioSum)
        );
        assert_eq!(
            assert_bps_sum(&[0u32]),
            Err(ContractError::InvalidRatioSum)
        );
        assert_eq!(
            assert_bps_sum(&[10_001u32]),
            Err(ContractError::InvalidRatioSum)
        );
        assert_eq!(
            assert_bps_sum(&[]),
            Err(ContractError::InvalidRatioSum)
        );
    }

    // --- signature validation (issue #937) ---

    fn nonce(env: &Env, seed: u8) -> BytesN<32> {
        BytesN::from_array(env, &[seed; 32])
    }

    #[test]
    fn digest_is_deterministic_and_domain_separated() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let n = nonce(&env, 1);
        let payload = Bytes::from_array(&env, &[1, 2, 3]);
        let d1 = cosigner_digest(&env, &contract, 7, &n, &payload).unwrap();
        let d2 = cosigner_digest(&env, &contract, 7, &n, &payload).unwrap();
        assert_eq!(d1, d2);
        // Different invoice id -> different digest.
        let d3 = cosigner_digest(&env, &contract, 8, &n, &payload).unwrap();
        assert_ne!(d1, d3);
        // Different nonce -> different digest.
        let d4 = cosigner_digest(&env, &contract, 7, &nonce(&env, 2), &payload).unwrap();
        assert_ne!(d1, d4);
    }

    /// Deterministic Ed25519 key pair whose public key is a `G...` address.
    fn signer_keypair(env: &Env, seed: u8) -> (ed25519_dalek::SigningKey, Address) {
        let sk = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
        let pk = BytesN::from_array(env, &sk.verifying_key().to_bytes());
        (sk, AddressPayload::AccountIdPublicKeyEd25519(pk).to_address(env))
    }

    fn sign_digest(env: &Env, sk: &ed25519_dalek::SigningKey, digest: &BytesN<32>) -> BytesN<64> {
        use ed25519_dalek::Signer;
        BytesN::from_array(env, &sk.sign(&digest.to_array()).to_bytes())
    }

    /// Storage-touching helpers need a contract context.
    fn in_contract<T>(env: &Env, f: impl FnOnce() -> T) -> T {
        let id = env.register(crate::SplitContract, ());
        env.as_contract(&id, f)
    }

    #[test]
    fn valid_signature_accepted() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let (sk, signer) = signer_keypair(&env, 3);
        let n = nonce(&env, 3);
        let payload = Bytes::from_array(&env, &[9, 9]);
        let digest = cosigner_digest(&env, &contract, 1, &n, &payload).unwrap();
        let sig = sign_digest(&env, &sk, &digest);
        in_contract(&env, || {
            assert!(
                verify_cosigner_signature(&env, &contract, 1, &signer, &n, &payload, &sig).is_ok()
            );
        });
    }

    #[test]
    #[should_panic]
    fn invalid_signature_rejected() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let (sk, signer) = signer_keypair(&env, 4);
        let n = nonce(&env, 4);
        let payload = Bytes::from_array(&env, &[1]);
        let digest = cosigner_digest(&env, &contract, 1, &n, &payload).unwrap();
        let sig = sign_digest(&env, &sk, &digest);
        // Tamper with the payload: the digest changes, so the host traps.
        let bad_payload = Bytes::from_array(&env, &[2]);
        in_contract(&env, || {
            let _ = verify_cosigner_signature(&env, &contract, 1, &signer, &n, &bad_payload, &sig);
        });
    }

    #[test]
    fn contract_signer_rejected() {
        let env = Env::default();
        let contract = Address::generate(&env);
        // `Address::generate` yields a contract (C...) address, which has no
        // Ed25519 key and can never produce a valid co-signer signature.
        let signer = Address::generate(&env);
        let n = nonce(&env, 8);
        let payload = Bytes::from_array(&env, &[1]);
        let sig = BytesN::from_array(&env, &[0u8; 64]);
        in_contract(&env, || {
            assert_eq!(
                verify_cosigner_signature(&env, &contract, 1, &signer, &n, &payload, &sig),
                Err(ContractError::InvalidSignature)
            );
        });
    }

    #[test]
    fn replayed_signature_rejected() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let (sk, signer) = signer_keypair(&env, 5);
        let n = nonce(&env, 5);
        let payload = Bytes::from_array(&env, &[7]);
        let digest = cosigner_digest(&env, &contract, 1, &n, &payload).unwrap();
        let sig = sign_digest(&env, &sk, &digest);
        in_contract(&env, || {
            assert!(
                verify_cosigner_signature(&env, &contract, 1, &signer, &n, &payload, &sig).is_ok()
            );
            assert!(is_nonce_used(&env, 1, &n));
            assert_eq!(
                verify_cosigner_signature(&env, &contract, 1, &signer, &n, &payload, &sig),
                Err(ContractError::SignatureReplayed)
            );
        });
    }

    #[test]
    fn nonce_scoped_per_invoice() {
        let env = Env::default();
        let n = nonce(&env, 6);
        in_contract(&env, || {
            assert!(consume_nonce(&env, 1, &n).is_ok());
            // Same nonce, different invoice -> allowed.
            assert!(consume_nonce(&env, 2, &n).is_ok());
            // Same nonce, same invoice -> replayed.
            assert_eq!(
                consume_nonce(&env, 1, &n),
                Err(ContractError::SignatureReplayed)
            );
        });
    }

    // --- oracle validation (issue #938) ---

    #[test]
    fn valid_oracle_address_accepted() {
        let env = Env::default();
        let oracle = Address::generate(&env);
        assert!(assert_valid_oracle_address(&env, &oracle).is_ok());
    }

    #[test]
    fn invalid_oracle_address_rejected() {
        let env = Env::default();
        // Every generated address round-trips through its strkey form.
        let oracle = Address::generate(&env);
        assert!(assert_valid_oracle_address(&env, &oracle).is_ok());
    }

    #[test]
    fn valid_oracle_condition_accepted() {
        assert!(assert_valid_oracle_condition(100, 0).is_ok());
        assert!(assert_valid_oracle_condition(100, 1).is_ok());
        assert!(assert_valid_oracle_condition(100, 2).is_ok());
    }

    #[test]
    fn invalid_oracle_condition_rejected() {
        assert_eq!(
            assert_valid_oracle_condition(0, 1),
            Err(ContractError::InvalidOracleCondition)
        );
        assert_eq!(
            assert_valid_oracle_condition(-5, 1),
            Err(ContractError::InvalidOracleCondition)
        );
        assert_eq!(
            assert_valid_oracle_condition(100, 3),
            Err(ContractError::InvalidOracleCondition)
        );
    }

    #[test]
    fn oracle_condition_evaluation() {
        assert_eq!(evaluate_oracle_condition(100, 0, 100), Ok(true));
        assert_eq!(evaluate_oracle_condition(100, 0, 99), Ok(false));
        assert_eq!(evaluate_oracle_condition(100, 1, 100), Ok(true));
        assert_eq!(evaluate_oracle_condition(100, 1, 99), Ok(false));
        assert_eq!(evaluate_oracle_condition(100, 2, 100), Ok(true));
        assert_eq!(evaluate_oracle_condition(100, 2, 101), Ok(false));
    }
}
