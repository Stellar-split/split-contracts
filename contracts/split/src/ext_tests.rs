#![cfg(test)]
//! Tests for recipient management, pause helpers, referrals and visibility tiers.

use super::test::{default_options, setup_initialized};
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};

fn mk(env: &Env, c: &SplitContractClient, token: &Address, creator: &Address) -> (u64, Address, Address) {
    let r1 = Address::generate(env);
    let r2 = Address::generate(env);
    let mut rs = Vec::new(env);
    rs.push_back(r1.clone());
    rs.push_back(r2.clone());
    let mut am = Vec::new(env);
    am.push_back(600_i128);
    am.push_back(400_i128);
    let id = c.create_invoice(creator, &rs, &am, token, &9_999_u64, &default_options(env));
    (id, r1, r2)
}

fn shares_sum(c: &SplitContractClient, id: u64) -> u32 {
    c.get_recipient_shares(&id).iter().map(|(_, b)| b).sum()
}

#[test]
fn recipients_add_remove_update_invariant() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    let creator = Address::generate(&env);
    let (id, r1, r2) = mk(&env, &c, &token, &creator);
    assert_eq!(shares_sum(&c, id), 10_000);

    c.remove_invoice_recipient(&id, &creator, &r2);
    assert_eq!(shares_sum(&c, id), 6_000);
    let r3 = Address::generate(&env);
    c.add_invoice_recipient(&id, &creator, &r3, &4_000);
    assert_eq!(shares_sum(&c, id), 10_000);
    c.update_recipient_share(&id, &creator, &r1, &6_000);
    assert_eq!(c.get_invoice(&id).recipients.len(), 2);
}

#[test]
#[should_panic(expected = "total bps exceeds 10000")]
fn recipients_add_over_cap_rejected() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    let creator = Address::generate(&env);
    let (id, _, _) = mk(&env, &c, &token, &creator);
    c.add_invoice_recipient(&id, &creator, &Address::generate(&env), &1);
}

#[test]
#[should_panic(expected = "total bps must equal 10000")]
fn recipients_update_must_sum_10000() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    let creator = Address::generate(&env);
    let (id, r1, _) = mk(&env, &c, &token, &creator);
    c.update_recipient_share(&id, &creator, &r1, &5_000);
}

#[test]
#[should_panic(expected = "RecipientModificationAfterPayment")]
fn recipients_modify_after_payment_rejected() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    let creator = Address::generate(&env);
    let (id, r1, _) = mk(&env, &c, &token, &creator);
    let payer = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&payer, &100);
    c.pay(&payer, &id, &100, &1, &false, &false, &None);
    c.update_recipient_share(&id, &creator, &r1, &6_000);
}

#[test]
#[should_panic(expected = "RecipientSharesIncomplete")]
fn recipients_incomplete_shares_block_payment() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    let creator = Address::generate(&env);
    let (id, _, r2) = mk(&env, &c, &token, &creator);
    c.remove_invoice_recipient(&id, &creator, &r2);
    let payer = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&payer, &100);
    c.pay(&payer, &id, &100, &1, &false, &false, &None);
}

#[test]
fn pause_blocks_manual_resume_and_auto_resume() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    env.ledger().set_timestamp(100);
    let creator = Address::generate(&env);
    let (id, _, _) = mk(&env, &c, &token, &creator);
    let payer = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&payer, &300);
    let reason = soroban_sdk::String::from_str(&env, "maintenance");

    c.pause_invoice(&creator, &id, &reason, &None);
    assert!(c.is_invoice_paused(&id));
    assert!(c.try_pay(&payer, &id, &100, &1, &false, &false, &None).is_err());
    c.resume_invoice(&creator, &id);
    assert!(!c.is_invoice_paused(&id));
    c.pay(&payer, &id, &100, &2, &false, &false, &None);

    c.pause_invoice(&creator, &id, &reason, &Some(200));
    assert!(c.is_invoice_paused(&id));
    assert!(c.try_pay(&payer, &id, &100, &3, &false, &false, &None).is_err());
    env.ledger().set_timestamp(200);
    assert!(!c.is_invoice_paused(&id));
    c.pay(&payer, &id, &100, &4, &false, &false, &None);
}

#[test]
fn referral_credits_and_claims() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    let creator = Address::generate(&env);
    let (id, _, _) = mk(&env, &c, &token, &creator);
    let payer = Address::generate(&env);
    let referrer = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&payer, &1_000);
    c.set_referral_fee_bps(&creator, &id, &1_000);

    c.pay_with_referrer(&payer, &id, &500, &1, &Some(referrer.clone()));
    assert_eq!(c.get_referral_balance(&referrer), 50);
    assert_eq!(c.claim_referral_rewards(&referrer), 50);
    assert_eq!(c.get_referral_balance(&referrer), 0);
    assert_eq!(soroban_sdk::token::Client::new(&env, &token).balance(&referrer), 50);

    // No referrer, no fee.
    c.pay_with_referrer(&payer, &id, &100, &2, &None);
    assert_eq!(c.get_referral_balance(&referrer), 0);
}

#[test]
#[should_panic(expected = "referral_fee_bps exceeds 1000")]
fn referral_fee_capped() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    let creator = Address::generate(&env);
    let (id, _, _) = mk(&env, &c, &token, &creator);
    c.set_referral_fee_bps(&creator, &id, &1_001);
}

fn code(env: &Env) -> soroban_sdk::Bytes {
    soroban_sdk::Bytes::from_slice(env, b"s3cret")
}

fn code_hash(env: &Env) -> soroban_sdk::BytesN<32> {
    env.crypto().sha256(&code(env)).into()
}

#[test]
fn visibility_public_pays_freely_and_tier_readable() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    let creator = Address::generate(&env);
    let (id, _, _) = mk(&env, &c, &token, &creator);
    assert_eq!(c.get_invoice_visibility(&id), soroban_sdk::symbol_short!("public"));
    let payer = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&payer, &100);
    c.pay(&payer, &id, &100, &1, &false, &false, &None);
}

#[test]
fn visibility_private_code_checked() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    let creator = Address::generate(&env);
    let (id, _, _) = mk(&env, &c, &token, &creator);
    c.set_invoice_visibility(&creator, &id, &InvoiceVisibility::Private(code_hash(&env)));
    assert_eq!(c.get_invoice_visibility(&id), soroban_sdk::symbol_short!("private"));
    let payer = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&payer, &200);
    let wrong = soroban_sdk::Bytes::from_slice(&env, b"nope");
    assert!(c.try_pay_with_access_code(&payer, &id, &100, &1, &Some(wrong)).is_err());
    assert!(c.try_pay(&payer, &id, &100, &2, &false, &false, &None).is_err());
    c.pay_with_access_code(&payer, &id, &100, &3, &Some(code(&env)));
}

#[test]
fn visibility_invite_only_without_whitelist_rejected() {
    let (env, cid, token) = setup_initialized();
    let c = SplitContractClient::new(&env, &cid);
    let creator = Address::generate(&env);
    let (id, _, _) = mk(&env, &c, &token, &creator);
    c.set_invoice_visibility(&creator, &id, &InvoiceVisibility::InviteOnly);
    let payer = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&payer, &100);
    assert!(c.try_pay(&payer, &id, &100, &1, &false, &false, &None).is_err());
}
