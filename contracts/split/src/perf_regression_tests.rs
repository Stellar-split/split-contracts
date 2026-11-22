#![cfg(test)]

//! Performance regression testing (#1027): records CPU cost per operation,
//! compares it against baselines with a tolerance, tracks the history,
//! analyses trends and raises alerts when a regression is detected.

use crate::testing_support::*;
use soroban_sdk::{testutils::Address as _, Address};

/// Soroban's per-transaction CPU instruction limit. Used as the absolute
/// baseline ceiling for every operation.
const NETWORK_CPU_LIMIT: u64 = 100_000_000;
/// Allowed growth over a baseline before a measurement counts as a regression.
const TOLERANCE_PCT: u64 = 10;
/// Maximum allowed cost ratio between a 20-recipient and a 1-recipient pay.
const MAX_SCALING_RATIO: u64 = 25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trend {
    Improving,
    Stable,
    Degrading,
}

#[derive(Debug, PartialEq, Eq)]
struct Alert {
    operation: String,
    baseline: u64,
    measured: u64,
}

/// Keeps the measurement history of each operation.
#[derive(Default)]
struct PerfTracker {
    history: std::collections::BTreeMap<String, std::vec::Vec<u64>>,
}

impl PerfTracker {
    fn record(&mut self, operation: &str, cpu: u64) {
        self.history.entry(operation.to_string()).or_default().push(cpu);
    }

    fn latest(&self, operation: &str) -> Option<u64> {
        self.history.get(operation).and_then(|h| h.last().copied())
    }

    /// Regression detection: measured exceeds `baseline` by more than the tolerance.
    fn detect_regression(operation: &str, baseline: u64, measured: u64) -> Option<Alert> {
        let allowed = baseline + baseline * TOLERANCE_PCT / 100;
        if measured > allowed {
            Some(Alert { operation: operation.to_string(), baseline, measured })
        } else {
            None
        }
    }

    /// Trend analysis: compares the mean of the older half with the newer half.
    fn trend(&self, operation: &str) -> Trend {
        let h = match self.history.get(operation) {
            Some(h) if h.len() >= 2 => h,
            _ => return Trend::Stable,
        };
        let mid = h.len() / 2;
        let mean = |s: &[u64]| s.iter().sum::<u64>() / s.len() as u64;
        let (old, new) = (mean(&h[..mid]), mean(&h[mid..]));
        if new > old + old * TOLERANCE_PCT / 100 {
            Trend::Degrading
        } else if new + new * TOLERANCE_PCT / 100 < old {
            Trend::Improving
        } else {
            Trend::Stable
        }
    }
}

fn cpu_used(env: &soroban_sdk::Env) -> u64 {
    env.cost_estimate().budget().cpu_instruction_cost()
}

/// Measure the CPU cost of one closure on a freshly reset budget.
fn measure(env: &soroban_sdk::Env, f: impl FnOnce()) -> u64 {
    env.cost_estimate().budget().reset_unlimited();
    f();
    cpu_used(env)
}

#[test]
fn regression_detector_flags_only_growth_beyond_tolerance() {
    assert_eq!(PerfTracker::detect_regression("pay", 1_000, 1_000), None);
    assert_eq!(PerfTracker::detect_regression("pay", 1_000, 1_100), None);
    assert_eq!(
        PerfTracker::detect_regression("pay", 1_000, 1_101),
        Some(Alert { operation: "pay".to_string(), baseline: 1_000, measured: 1_101 })
    );
    assert_eq!(PerfTracker::detect_regression("pay", 1_000, 500), None);
}

#[test]
fn trend_analysis_identifies_direction() {
    let mut t = PerfTracker::default();
    for v in [100, 100, 150, 160] {
        t.record("degrading", v);
    }
    for v in [200, 190, 100, 90] {
        t.record("improving", v);
    }
    for v in [100, 102, 99, 101] {
        t.record("stable", v);
    }
    t.record("single", 100);

    assert_eq!(t.trend("degrading"), Trend::Degrading);
    assert_eq!(t.trend("improving"), Trend::Improving);
    assert_eq!(t.trend("stable"), Trend::Stable);
    assert_eq!(t.trend("single"), Trend::Stable);
    assert_eq!(t.trend("missing"), Trend::Stable);
    assert_eq!(t.latest("stable"), Some(101));
}

#[test]
fn core_operations_stay_within_baselines() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let mut tracker = PerfTracker::default();

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);
    fund(&env, &token_id, &payer, 1_000);

    let mut recipients = soroban_sdk::Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = soroban_sdk::Vec::new(&env);
    amounts.push_back(100_i128);

    let mut id = 0;
    let create_cost = measure(&env, || {
        id = c.create_invoice(
            &creator,
            &recipients,
            &amounts,
            &token_id,
            &DEADLINE,
            &crate::test::default_options(&env),
        );
    });
    tracker.record("create_invoice", create_cost);

    let pay_cost = measure(&env, || {
        c.pay(&payer, &id, &100_i128, &0_u64, &false, &false, &None);
    });
    tracker.record("pay", pay_cost);

    let get_cost = measure(&env, || {
        let _ = c.get_invoice(&id);
    });
    tracker.record("get_invoice", get_cost);

    let mut alerts = std::vec::Vec::new();
    for op in ["create_invoice", "pay", "get_invoice"] {
        let measured = tracker.latest(op).unwrap();
        assert!(measured > 0, "{op} must consume measurable CPU");
        if let Some(alert) = PerfTracker::detect_regression(op, NETWORK_CPU_LIMIT, measured) {
            alerts.push(alert);
        }
    }
    assert!(alerts.is_empty(), "performance regressions: {alerts:?}");
}

#[test]
fn pay_cost_scales_sub_quadratically_with_recipients() {
    let cost_for = |n: u32| -> u64 {
        let (env, contract_id, token_id) = setup();
        let c = client(&env, &contract_id);
        let (id, _) = multi_invoice(&env, &c, &token_id, n, 10);
        let payer = Address::generate(&env);
        fund(&env, &token_id, &payer, 10 * n as i128);
        measure(&env, || {
            c.pay(&payer, &id, &(10 * n as i128), &0_u64, &false, &false, &None);
        })
    };

    let one = cost_for(1);
    let twenty = cost_for(20);
    assert!(one > 0);
    assert!(
        twenty <= one * MAX_SCALING_RATIO,
        "paying 20 recipients cost {twenty}, more than {MAX_SCALING_RATIO}x the 1-recipient cost {one}"
    );
}
