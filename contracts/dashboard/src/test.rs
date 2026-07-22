#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let split_id = Address::generate(&env);
    let dashboard_id = env.register(DashboardContract, ());

    let client = DashboardContractClient::new(&env, &dashboard_id);
    client.initialize(&split_id);

    (env, split_id, dashboard_id)
}

#[test]
fn test_initial_dashboard_empty() {
    let (env, _split_id, dashboard_id) = setup();
    let client = DashboardContractClient::new(&env, &dashboard_id);

    let creator = Address::generate(&env);
    let dash = client.get_creator_dashboard(&creator);

    assert_eq!(dash.invoice_count, 0);
    assert_eq!(dash.total_volume, 0);
    assert_eq!(dash.released_count, 0);
    assert_eq!(dash.refunded_count, 0);
    assert_eq!(dash.released_volume, 0);
}

#[test]
fn test_record_created_updates_stats() {
    let (env, _split_id, dashboard_id) = setup();
    let client = DashboardContractClient::new(&env, &dashboard_id);

    let creator = Address::generate(&env);

    client.record_created(&creator, &1000_i128);
    client.record_created(&creator, &2000_i128);

    let dash = client.get_creator_dashboard(&creator);
    assert_eq!(dash.invoice_count, 2);
    assert_eq!(dash.total_volume, 3000);
    assert_eq!(dash.released_count, 0);
    assert_eq!(dash.refunded_count, 0);
    assert_eq!(dash.released_volume, 0);
}

#[test]
fn test_record_released_updates_stats() {
    let (env, _split_id, dashboard_id) = setup();
    let client = DashboardContractClient::new(&env, &dashboard_id);

    let creator = Address::generate(&env);

    client.record_created(&creator, &5000_i128);
    client.record_released(&creator, &3000_i128);
    client.record_released(&creator, &2000_i128);

    let dash = client.get_creator_dashboard(&creator);
    assert_eq!(dash.invoice_count, 1);
    assert_eq!(dash.total_volume, 5000);
    assert_eq!(dash.released_count, 2);
    assert_eq!(dash.refunded_count, 0);
    assert_eq!(dash.released_volume, 5000);
}

#[test]
fn test_record_refunded_updates_stats() {
    let (env, _split_id, dashboard_id) = setup();
    let client = DashboardContractClient::new(&env, &dashboard_id);

    let creator = Address::generate(&env);

    client.record_created(&creator, &1000_i128);
    client.record_refunded(&creator);

    let dash = client.get_creator_dashboard(&creator);
    assert_eq!(dash.invoice_count, 1);
    assert_eq!(dash.total_volume, 1000);
    assert_eq!(dash.released_count, 0);
    assert_eq!(dash.refunded_count, 1);
    assert_eq!(dash.released_volume, 0);
}

#[test]
fn test_stats_are_per_creator() {
    let (env, _split_id, dashboard_id) = setup();
    let client = DashboardContractClient::new(&env, &dashboard_id);

    let creator_a = Address::generate(&env);
    let creator_b = Address::generate(&env);

    client.record_created(&creator_a, &100_i128);
    client.record_created(&creator_a, &200_i128);
    client.record_created(&creator_b, &500_i128);
    client.record_released(&creator_a, &300_i128);
    client.record_refunded(&creator_b);

    let dash_a = client.get_creator_dashboard(&creator_a);
    assert_eq!(dash_a.invoice_count, 2);
    assert_eq!(dash_a.total_volume, 300);
    assert_eq!(dash_a.released_count, 1);
    assert_eq!(dash_a.refunded_count, 0);
    assert_eq!(dash_a.released_volume, 300);

    let dash_b = client.get_creator_dashboard(&creator_b);
    assert_eq!(dash_b.invoice_count, 1);
    assert_eq!(dash_b.total_volume, 500);
    assert_eq!(dash_b.released_count, 0);
    assert_eq!(dash_b.refunded_count, 1);
    assert_eq!(dash_b.released_volume, 0);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_unauthorized_caller_rejected() {
    let env = Env::default();

    let split_id = Address::generate(&env);
    let dashboard_id = env.register(DashboardContract, ());

    let client = DashboardContractClient::new(&env, &dashboard_id);
    client.initialize(&split_id);

    let creator = Address::generate(&env);

    // Call without mock_all_auths — require_auth() on the authorized address will fail
    // because the test caller is not the authorized split contract.
    DashboardContractClient::new(&env, &dashboard_id).record_created(&creator, &100_i128);
}
