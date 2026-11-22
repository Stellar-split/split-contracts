#![cfg(test)]

extern crate std;

use crate::{
    CharityInvoiceStatus, CharitySplitContract, CharitySplitContractClient, Error, MAX_DONATION_BPS,
};
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{symbol_short, token, vec, Address, Env, Symbol, TryFromVal, Val, Vec};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct Setup {
    env: Env,
    client: CharitySplitContractClient<'static>,
    admin: Address,
    charity: Address,
    token: Address,
    creator: Address,
    alice: Address,
    bob: Address,
    payer: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CharitySplitContract, ());
    let client = CharitySplitContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let charity = Address::generate(&env);
    client.initialize(&admin);
    client.register_charity(&admin, &charity, &symbol_short!("redcross"));

    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();
    let payer = Address::generate(&env);
    token::StellarAssetClient::new(&env, &token).mint(&payer, &1_000_000);

    Setup {
        creator: Address::generate(&env),
        alice: Address::generate(&env),
        bob: Address::generate(&env),
        env,
        client,
        admin,
        charity,
        token,
        payer,
    }
}

/// Invoice of 6_000 + 4_000 to alice and bob, donating `bps` to the charity.
fn create_invoice(s: &Setup, bps: u32) -> u64 {
    s.client.create_invoice(
        &s.creator,
        &s.token,
        &vec![&s.env, s.alice.clone(), s.bob.clone()],
        &vec![&s.env, 6_000_i128, 4_000_i128],
        &s.charity,
        &bps,
    )
}

fn balance(s: &Setup, who: &Address) -> i128 {
    token::Client::new(&s.env, &s.token).balance(who)
}

/// Returns true if an event with topics `(charity, action, ..)` was emitted.
fn has_event(env: &Env, action: Symbol) -> bool {
    for event in env.events().all().events().iter() {
        let soroban_sdk::xdr::ContractEventBody::V0(body) = &event.body;
        let topics: &[soroban_sdk::xdr::ScVal] = body.topics.as_ref();
        if topics.len() < 2 {
            continue;
        }
        let t0 = Val::try_from_val(env, &topics[0]).ok();
        let t1 = Val::try_from_val(env, &topics[1]).ok();
        if let (Some(t0), Some(t1)) = (t0, t1) {
            let s0 = Symbol::try_from_val(env, &t0).ok();
            let s1 = Symbol::try_from_val(env, &t1).ok();
            if s0 == Some(symbol_short!("charity")) && s1 == Some(action.clone()) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Charity registry
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_twice_fails() {
    let s = setup();
    assert_eq!(s.client.try_initialize(&s.admin), Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_register_charity() {
    let s = setup();
    let c = Address::generate(&s.env);
    s.client.register_charity(&s.admin, &c, &symbol_short!("unicef"));
    assert!(has_event(&s.env, symbol_short!("reg")));
    let record = s.client.get_charity(&c).unwrap();
    assert_eq!(record.name, symbol_short!("unicef"));
    assert!(record.active);
    assert_eq!(record.donation_count, 0);
}

#[test]
fn test_register_charity_twice_fails() {
    let s = setup();
    assert_eq!(
        s.client.try_register_charity(&s.admin, &s.charity, &symbol_short!("dup")),
        Err(Ok(Error::CharityAlreadyRegistered))
    );
}

#[test]
fn test_non_admin_cannot_register_charity() {
    let s = setup();
    let c = Address::generate(&s.env);
    assert_eq!(
        s.client.try_register_charity(&s.creator, &c, &symbol_short!("x")),
        Err(Ok(Error::NotAdmin))
    );
}

#[test]
fn test_deactivated_charity_rejected_for_new_invoice() {
    let s = setup();
    s.client.set_charity_active(&s.admin, &s.charity, &false);
    assert!(has_event(&s.env, symbol_short!("status")));
    assert_eq!(
        s.client.try_create_invoice(
            &s.creator,
            &s.token,
            &vec![&s.env, s.alice.clone()],
            &vec![&s.env, 1_000_i128],
            &s.charity,
            &500,
        ),
        Err(Ok(Error::CharityInactive))
    );
}

// ---------------------------------------------------------------------------
// Invoice creation
// ---------------------------------------------------------------------------

#[test]
fn test_create_invoice_success() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    assert!(has_event(&s.env, symbol_short!("created")));
    let inv = s.client.get_invoice(&id).unwrap();
    assert_eq!(inv.total, 10_000);
    assert_eq!(inv.funded, 0);
    assert_eq!(inv.donation_bps, 1_000);
    assert_eq!(inv.status, CharityInvoiceStatus::Open);
}

#[test]
fn test_create_invoice_invalid_bps_fails() {
    let s = setup();
    assert_eq!(
        s.client.try_create_invoice(
            &s.creator,
            &s.token,
            &vec![&s.env, s.alice.clone()],
            &vec![&s.env, 1_000_i128],
            &s.charity,
            &0,
        ),
        Err(Ok(Error::InvalidDonationBps))
    );
    assert_eq!(
        s.client.try_create_invoice(
            &s.creator,
            &s.token,
            &vec![&s.env, s.alice.clone()],
            &vec![&s.env, 1_000_i128],
            &s.charity,
            &(MAX_DONATION_BPS + 1),
        ),
        Err(Ok(Error::InvalidDonationBps))
    );
}

#[test]
fn test_create_invoice_mismatched_lists_fails() {
    let s = setup();
    assert_eq!(
        s.client.try_create_invoice(
            &s.creator,
            &s.token,
            &vec![&s.env, s.alice.clone(), s.bob.clone()],
            &vec![&s.env, 1_000_i128],
            &s.charity,
            &500,
        ),
        Err(Ok(Error::InvalidRecipients))
    );
    let empty_r: Vec<Address> = Vec::new(&s.env);
    let empty_a: Vec<i128> = Vec::new(&s.env);
    assert_eq!(
        s.client.try_create_invoice(&s.creator, &s.token, &empty_r, &empty_a, &s.charity, &500),
        Err(Ok(Error::InvalidRecipients))
    );
}

#[test]
fn test_create_invoice_non_positive_amount_fails() {
    let s = setup();
    assert_eq!(
        s.client.try_create_invoice(
            &s.creator,
            &s.token,
            &vec![&s.env, s.alice.clone()],
            &vec![&s.env, 0_i128],
            &s.charity,
            &500,
        ),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn test_charity_as_recipient_fails() {
    let s = setup();
    assert_eq!(
        s.client.try_create_invoice(
            &s.creator,
            &s.token,
            &vec![&s.env, s.charity.clone()],
            &vec![&s.env, 1_000_i128],
            &s.charity,
            &500,
        ),
        Err(Ok(Error::CharityIsRecipient))
    );
}

#[test]
fn test_unknown_charity_fails() {
    let s = setup();
    let unknown = Address::generate(&s.env);
    assert_eq!(
        s.client.try_create_invoice(
            &s.creator,
            &s.token,
            &vec![&s.env, s.alice.clone()],
            &vec![&s.env, 1_000_i128],
            &unknown,
            &500,
        ),
        Err(Ok(Error::CharityNotFound))
    );
}

// ---------------------------------------------------------------------------
// Payments & release
// ---------------------------------------------------------------------------

#[test]
fn test_preview_split() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    let split = s.client.preview_split(&id);
    assert_eq!(split.payouts, vec![&s.env, 5_400_i128, 3_600_i128]);
    assert_eq!(split.donation, 1_000);
}

#[test]
fn test_preview_split_rounding_goes_to_charity() {
    let s = setup();
    let id = s.client.create_invoice(
        &s.creator,
        &s.token,
        &vec![&s.env, s.alice.clone(), s.bob.clone()],
        &vec![&s.env, 333_i128, 333_i128],
        &s.charity,
        &250,
    );
    let split = s.client.preview_split(&id);
    // 333 * 2.5% = 8.325 → 8 deducted each; net 325 each.
    assert_eq!(split.payouts, vec![&s.env, 325_i128, 325_i128]);
    assert_eq!(split.donation, 16);
}

#[test]
fn test_partial_payment_holds_funds() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    let funded = s.client.pay(&s.payer, &id, &4_000);
    assert_eq!(funded, 4_000);
    assert!(has_event(&s.env, symbol_short!("paid")));
    assert_eq!(s.client.get_invoice(&id).unwrap().status, CharityInvoiceStatus::Open);
    assert_eq!(s.client.get_contribution(&id, &s.payer), 4_000);
    assert_eq!(balance(&s, &s.alice), 0);
}

#[test]
fn test_full_payment_releases_with_donation() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    s.client.pay(&s.payer, &id, &4_000);
    s.client.pay(&s.payer, &id, &6_000);
    assert!(has_event(&s.env, symbol_short!("released")));
    assert!(has_event(&s.env, symbol_short!("donated")));

    assert_eq!(balance(&s, &s.alice), 5_400);
    assert_eq!(balance(&s, &s.bob), 3_600);
    assert_eq!(balance(&s, &s.charity), 1_000);

    let inv = s.client.get_invoice(&id).unwrap();
    assert_eq!(inv.status, CharityInvoiceStatus::Released);
    assert_eq!(inv.donated, 1_000);
    assert_eq!(s.client.get_charity_total(&s.charity, &s.token), 1_000);
    assert_eq!(s.client.get_charity(&s.charity).unwrap().donation_count, 1);
}

#[test]
fn test_overpayment_fails() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    assert_eq!(s.client.try_pay(&s.payer, &id, &10_001), Err(Ok(Error::OverFunded)));
}

#[test]
fn test_pay_released_invoice_fails() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    s.client.pay(&s.payer, &id, &10_000);
    assert_eq!(s.client.try_pay(&s.payer, &id, &1), Err(Ok(Error::InvoiceNotOpen)));
}

#[test]
fn test_pay_zero_fails() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    assert_eq!(s.client.try_pay(&s.payer, &id, &0), Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_pay_unknown_invoice_fails() {
    let s = setup();
    assert_eq!(s.client.try_pay(&s.payer, &99, &1), Err(Ok(Error::InvoiceNotFound)));
}

// ---------------------------------------------------------------------------
// Extra donations
// ---------------------------------------------------------------------------

#[test]
fn test_donate_extra_goes_straight_to_charity() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    s.client.donate_extra(&s.payer, &id, &250);
    assert!(has_event(&s.env, symbol_short!("donated")));
    assert_eq!(balance(&s, &s.charity), 250);
    assert_eq!(s.client.get_donor_total(&s.payer, &s.token), 250);
    assert_eq!(s.client.get_charity_total(&s.charity, &s.token), 250);

    s.client.pay(&s.payer, &id, &10_000);
    assert_eq!(s.client.get_charity_total(&s.charity, &s.token), 1_250);
}

#[test]
fn test_donate_extra_on_cancelled_fails() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    s.client.cancel_invoice(&s.creator, &id);
    assert_eq!(
        s.client.try_donate_extra(&s.payer, &id, &100),
        Err(Ok(Error::InvoiceNotOpen))
    );
}

// ---------------------------------------------------------------------------
// Cancellation & refunds
// ---------------------------------------------------------------------------

#[test]
fn test_cancel_and_refund() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    s.client.pay(&s.payer, &id, &3_000);
    let before = balance(&s, &s.payer);

    s.client.cancel_invoice(&s.creator, &id);
    assert!(has_event(&s.env, symbol_short!("cancelled")));
    assert_eq!(s.client.get_invoice(&id).unwrap().status, CharityInvoiceStatus::Cancelled);

    let refunded = s.client.claim_refund(&s.payer, &id);
    assert!(has_event(&s.env, symbol_short!("refunded")));
    assert_eq!(refunded, 3_000);
    assert_eq!(balance(&s, &s.payer), before + 3_000);
    assert_eq!(s.client.get_contribution(&id, &s.payer), 0);
    assert_eq!(s.client.get_invoice(&id).unwrap().funded, 0);
}

#[test]
fn test_refund_twice_fails() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    s.client.pay(&s.payer, &id, &3_000);
    s.client.cancel_invoice(&s.creator, &id);
    s.client.claim_refund(&s.payer, &id);
    assert_eq!(
        s.client.try_claim_refund(&s.payer, &id),
        Err(Ok(Error::NothingToRefund))
    );
}

#[test]
fn test_refund_open_invoice_fails() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    s.client.pay(&s.payer, &id, &3_000);
    assert_eq!(s.client.try_claim_refund(&s.payer, &id), Err(Ok(Error::NotCancelled)));
}

#[test]
fn test_non_creator_cannot_cancel() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    assert_eq!(s.client.try_cancel_invoice(&s.payer, &id), Err(Ok(Error::NotCreator)));
}

#[test]
fn test_cannot_cancel_released_invoice() {
    let s = setup();
    let id = create_invoice(&s, 1_000);
    s.client.pay(&s.payer, &id, &10_000);
    assert_eq!(
        s.client.try_cancel_invoice(&s.creator, &id),
        Err(Ok(Error::InvoiceNotOpen))
    );
}
