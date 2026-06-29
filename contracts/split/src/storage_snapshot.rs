#![cfg(test)]

use super::*;
use soroban_sdk::{xdr::ToXdr, Env};

fn hex_xdr(env: &Env, val: impl ToXdr) -> String {
    let bytes = val.to_xdr(env);
    bytes.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().concat()
}

#[test]
fn storage_key_snapshot() {
    let env = Env::default();
    // Deterministic address (all-zero G... strkey) — do not change.
    let a = Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
    let s = symbol_short!("x");

    let mut keys: Vec<(&str, String)> = Vec::new();

    // -----------------------------------------------------------------------
    // Instance-tier keys (contract-level singletons, return Symbol)
    // -----------------------------------------------------------------------

    keys.push(("admin_key", hex_xdr(&env, admin_key())));
    keys.push(("admins_key", hex_xdr(&env, admins_key())));
    keys.push(("paused_key", hex_xdr(&env, paused_key())));
    keys.push(("paused_fns_key", hex_xdr(&env, paused_fns_key())));
    keys.push(("treasury_key", hex_xdr(&env, treasury_key())));
    keys.push(("usdc_token_key", hex_xdr(&env, usdc_token_key())));
    keys.push(("creation_fee_key", hex_xdr(&env, creation_fee_key())));
    keys.push(("platform_fee_bps_key", hex_xdr(&env, platform_fee_bps_key())));
    keys.push(("platform_fee_waiver_list_key", hex_xdr(&env, platform_fee_waiver_list_key())));
    keys.push(("creator_fee_waiver_key", hex_xdr(&env, creator_fee_waiver_key())));
    keys.push(("counter_key", hex_xdr(&env, counter_key())));
    keys.push(("global_payer_limit_key", hex_xdr(&env, global_payer_limit_key())));
    keys.push(("global_payer_window_key", hex_xdr(&env, global_payer_window_key())));
    keys.push(("stream_contract_key", hex_xdr(&env, stream_contract_key())));
    keys.push(("creator_whitelist_key", hex_xdr(&env, creator_whitelist_key())));
    keys.push(("compliance_key", hex_xdr(&env, compliance_key())));
    keys.push(("rate_limit_key", hex_xdr(&env, rate_limit_key())));
    keys.push(("rate_window_key", hex_xdr(&env, rate_window_key())));
    keys.push(("max_cancel_bps_key", hex_xdr(&env, max_cancel_bps_key())));
    keys.push(("receipt_factory_key", hex_xdr(&env, receipt_factory_key())));
    keys.push(("dashboard_contract_key", hex_xdr(&env, dashboard_contract_key())));
    keys.push(("nft_gate_key", hex_xdr(&env, nft_gate_key())));
    keys.push(("timelock_secs_key", hex_xdr(&env, timelock_secs_key())));
    keys.push(("timelock_action_counter_key", hex_xdr(&env, timelock_action_counter_key())));
    keys.push(("fee_tiers_key", hex_xdr(&env, fee_tiers_key())));
    keys.push(("pending_admin_key", hex_xdr(&env, pending_admin_key())));
    keys.push(("governance_contract_key", hex_xdr(&env, governance_contract_key())));
    keys.push(("factories_key", hex_xdr(&env, factories_key())));
    keys.push(("total_invoices_key", hex_xdr(&env, total_invoices_key())));
    keys.push(("total_volume_key", hex_xdr(&env, total_volume_key())));
    keys.push(("total_released_key", hex_xdr(&env, total_released_key())));
    keys.push(("total_refunded_key", hex_xdr(&env, total_refunded_key())));
    keys.push(("treasury_group_counter_key", hex_xdr(&env, treasury_group_counter_key())));
    keys.push(("circuit_breaker_key", hex_xdr(&env, circuit_breaker_key())));
    keys.push(("circuit_breaker_reason_key", hex_xdr(&env, circuit_breaker_reason_key())));
    keys.push(("archive_after_ledgers_key", hex_xdr(&env, archive_after_ledgers_key())));
    keys.push(("platform_vol_thresh_key", hex_xdr(&env, platform_vol_thresh_key())));
    keys.push(("platform_vol_mile_key", hex_xdr(&env, platform_vol_mile_key())));
    keys.push(("creator_vol_thresh_key", hex_xdr(&env, creator_vol_thresh_key())));
    keys.push(("kyc_contract_key", hex_xdr(&env, kyc_contract_key())));
    keys.push(("upgrade_proposal_key", hex_xdr(&env, upgrade_proposal_key())));

    // -----------------------------------------------------------------------
    // Persistent-tier keys (per-entity)
    // -----------------------------------------------------------------------

    // (Symbol, u64)
    keys.push(("invoice_key", hex_xdr(&env, invoice_key(1))));
    keys.push(("invoice_ext_key", hex_xdr(&env, invoice_ext_key(1))));
    keys.push(("invoice_ext2_key", hex_xdr(&env, invoice_ext2_key(1))));
    keys.push(("invoice_compact_key", hex_xdr(&env, invoice_compact_key(1))));
    keys.push(("invoice_hot_key", hex_xdr(&env, invoice_hot_key(1))));
    keys.push(("audit_log_key", hex_xdr(&env, audit_log_key(1))));
    keys.push(("archive_marker_key", hex_xdr(&env, archive_marker_key(1))));
    keys.push(("created_ledger_key", hex_xdr(&env, created_ledger_key(1))));
    keys.push(("subscription_params_key", hex_xdr(&env, subscription_params_key(1))));
    keys.push(("confidential_count_key", hex_xdr(&env, confidential_count_key(1))));
    keys.push(("ext_vote_key", hex_xdr(&env, ext_vote_key(1))));
    keys.push(("group_key", hex_xdr(&env, group_key(1))));
    keys.push(("invoice_group_key", hex_xdr(&env, invoice_group_key(1))));
    keys.push(("invoice_treasury_key", hex_xdr(&env, invoice_treasury_key(1))));
    keys.push(("group_treasury_key", hex_xdr(&env, group_treasury_key(1))));
    keys.push(("delegate_key", hex_xdr(&env, delegate_key(1))));
    keys.push(("payment_window_key", hex_xdr(&env, payment_window_key(1))));
    keys.push(("cert_key", hex_xdr(&env, cert_key(1))));
    keys.push(("timelock_action_key", hex_xdr(&env, timelock_action_key(1))));
    keys.push(("refunded_key", hex_xdr(&env, refunded_key(1))));

    // (Symbol, Address)
    keys.push(("pause_exempt_key", hex_xdr(&env, pause_exempt_key(&a))));
    keys.push(("global_vel_key", hex_xdr(&env, global_vel_key(&a))));
    keys.push(("rep_key", hex_xdr(&env, rep_key(&a))));
    keys.push(("credit_key", hex_xdr(&env, credit_key(&a))));
    keys.push(("referral_count_key", hex_xdr(&env, referral_count_key(&a))));
    keys.push(("recipient_invoice_ids_key", hex_xdr(&env, recipient_invoice_ids_key(&a))));
    keys.push(("delegate_pay_key", hex_xdr(&env, delegate_pay_key(&a))));
    keys.push(("rate_usage_key", hex_xdr(&env, rate_usage_key(&a))));
    keys.push(("invoice_count_key", hex_xdr(&env, invoice_count_key(&a))));
    keys.push(("cancel_count_key", hex_xdr(&env, cancel_count_key(&a))));
    keys.push(("creator_stats_count_key", hex_xdr(&env, creator_stats_count_key(&a))));
    keys.push(("creator_stats_volume_key", hex_xdr(&env, creator_stats_volume_key(&a))));
    keys.push(("creator_stats_released_key", hex_xdr(&env, creator_stats_released_key(&a))));
    keys.push(("creator_stats_refunded_key", hex_xdr(&env, creator_stats_refunded_key(&a))));
    keys.push(("creator_stats_payers_key", hex_xdr(&env, creator_stats_payers_key(&a))));
    keys.push(("creator_stats_avg_funding_key", hex_xdr(&env, creator_stats_avg_funding_key(&a))));
    keys.push(("creator_vol_mile_key", hex_xdr(&env, creator_vol_mile_key(&a))));
    keys.push(("creator_volume_cap_key", hex_xdr(&env, creator_volume_cap_key(&a))));
    keys.push(("creator_volume_used_key", hex_xdr(&env, creator_volume_used_key(&a))));

    // (Symbol, u64, Address)
    keys.push(("confidential_pay_key", hex_xdr(&env, confidential_pay_key(1, &a))));
    keys.push(("reminder_key", hex_xdr(&env, reminder_key(1, &a))));
    keys.push(("pending_payout_key", hex_xdr(&env, pending_payout_key(1, &a))));
    keys.push(("channel_key", hex_xdr(&env, channel_key(1, &a))));
    keys.push(("nonce_key", hex_xdr(&env, nonce_key(1, &a))));
    keys.push(("vel_key", hex_xdr(&env, vel_key(1, &a))));
    keys.push(("receipt_token_key", hex_xdr(&env, receipt_token_key(1, &a))));
    keys.push(("accum_key", hex_xdr(&env, accum_key(1, &a))));

    // (Symbol, u64, Address) where Address is owned
    keys.push(("payer_cooldown_key", hex_xdr(&env, payer_cooldown_key(1, a.clone()))));

    // (Symbol, u64, u64)
    keys.push(("pay_shard_key", hex_xdr(&env, pay_shard_key(1, 1))));

    // Issue #332: recipient optimisation keys (Symbol, u64)
    keys.push(("recipients_list_key", hex_xdr(&env, recipients_list_key(1))));
    keys.push(("amounts_list_key", hex_xdr(&env, amounts_list_key(1))));
    keys.push(("paid_flags_key", hex_xdr(&env, paid_flags_key(1))));

    // Issue #333: milestone flags key (Symbol, u64) — instance storage
    keys.push(("milestone_flags_key", hex_xdr(&env, milestone_flags_key(1))));

    // Issue #334: compact XDR overlay keys (Symbol, u64)
    keys.push(("compact_status_key", hex_xdr(&env, compact_status_key(1))));
    keys.push(("compact_deadline_ledger_key", hex_xdr(&env, compact_deadline_ledger_key(1))));

    // (Symbol, Address, Symbol)
    keys.push(("template_key", hex_xdr(&env, template_key(&a, &s))));
    keys.push(("template_version_count_key", hex_xdr(&env, template_version_count_key(&a, &s))));

    // (Symbol, Address, Symbol, u32)
    keys.push(("template_version_key", hex_xdr(&env, template_version_key(&a, &s, 1))));

    // Sort by key name for deterministic output
    keys.sort_by(|a, b| a.0.cmp(b.0));

    // -----------------------------------------------------------------------
    // Collision check
    // -----------------------------------------------------------------------
    {
        let mut seen = std::collections::HashSet::new();
        for (name, xdr) in &keys {
            assert!(
                seen.insert(xdr),
                "Collision detected: '{}' has the same XDR as another key",
                name,
            );
        }
    }

    // -----------------------------------------------------------------------
    // Build expected snapshot JSON
    // -----------------------------------------------------------------------
    let mut lines: Vec<String> = Vec::new();
    lines.push("{".to_string());
    lines.push(format!("  \"_comment\": \"Storage key XDR snapshot — see README Storage Key Registry section for policy.\","));
    lines.push(format!("  \"_keys_introduced\": \"Snapshot introduced with #331. Add your key name and XDR here.\","));
    lines.push(format!("  \"version\": \"1\","));
    lines.push(format!("  \"keys\": {{"));
    for (i, (name, xdr)) in keys.iter().enumerate() {
        let comma = if i == keys.len() - 1 { "" } else { "," };
        lines.push(format!("    \"{}\": \"{}\"{}", name, xdr, comma));
    }
    lines.push(format!("  }}"));
    lines.push(format!("}}"));
    lines.push(String::new());
    let generated = lines.join("\n");

    // -----------------------------------------------------------------------
    // Compare against committed baseline
    // -----------------------------------------------------------------------
    let baseline = include_str!("../../../tests/snapshots/storage_keys.json");

    if generated != baseline {
        panic!(
            "\n=== SNAPSHOT MISMATCH ===\n\
             Storage key XDR has changed from the committed baseline.\n\
             If this is intentional, update tests/snapshots/storage_keys.json\n\
             with the new output below, and include a migration note in your PR.\n\
             \n\
             === EXPECTED (generated) ===\n\
             {}\n\
             === ACTUAL (baseline) ===\n\
             {}\n\
             ============================",
            generated, baseline,
        );
    }
}
