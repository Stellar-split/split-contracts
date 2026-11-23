//! Built-in Invoice Template Library with customization, management, and usage tracking.
//!
//! Provides a curated set of built-in invoice templates (payroll, revenue share,
//! crowdfunding, freelance, treasury) alongside custom template registration,
//! flexible per-instantiation customization, and on-chain usage analytics.

use soroban_sdk::{contracttype, symbol_short, Address, Env, String, Symbol, Vec};

use crate::validation;

/// Basis-point denominator (100% = 10_000 bps).
pub const BPS_DENOMINATOR: u32 = 10_000;

/// Maximum number of templates allowed in the library to bound storage iteration.
pub const MAX_LIBRARY_TEMPLATES: u32 = 100;

/// Category for organizing and filtering invoice templates.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum TemplateCategory {
    /// Team salary and recurring contractor payroll splits.
    Payroll = 0,
    /// Creator, partner, or affiliate revenue sharing splits.
    RevenueShare = 1,
    /// Multi-backer crowdfunding campaigns with funding thresholds.
    Crowdfunding = 2,
    /// Client-to-freelancer retainers and milestone invoices.
    Freelance = 3,
    /// DAO and organizational treasury disbursements.
    Treasury = 4,
    /// User-defined custom template category.
    Custom = 5,
}

/// A template stored in the invoice template library.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryTemplate {
    /// Unique short identifier for the template (e.g., `equal2`, `rev7030`).
    pub id: Symbol,
    /// Human-readable display name.
    pub name: String,
    /// Concise description of the template's intended use case.
    pub description: String,
    /// Organizational category.
    pub category: TemplateCategory,
    /// Default split ratios in basis points (must sum to `10_000`).
    pub default_ratios: Vec<u32>,
    /// Default deadline duration in seconds from invoice creation.
    pub default_deadline_secs: u64,
    /// Default early withdrawal setting.
    pub allow_early_withdrawal: bool,
    /// Default minimum funding threshold in basis points (`0..=10_000`).
    pub min_funding_bps: u32,
    /// Default creator fee in basis points (`0..=10_000`).
    pub creator_fee_bps: u32,
    /// `true` for built-in library templates, `false` for custom templates.
    pub is_builtin: bool,
    /// Whether the template is active and available for instantiation.
    pub enabled: bool,
    /// Monotonic version counter incremented on each update.
    pub version: u32,
    /// `None` for built-in templates, `Some(address)` for custom templates.
    pub created_by: Option<Address>,
    /// Ledger timestamp when the template was registered.
    pub created_at: u64,
    /// Ledger timestamp when the template was last updated.
    pub updated_at: u64,
}

/// Optional per-instantiation overrides when creating an invoice from a library template.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateCustomization {
    /// Override the template's default split ratios (must sum to `10_000`).
    pub ratios: Option<Vec<u32>>,
    /// Override the explicit deadline timestamp (otherwise `now + default_deadline_secs`).
    pub deadline: Option<u64>,
    /// Override `allow_early_withdrawal`.
    pub allow_early_withdrawal: Option<bool>,
    /// Override `min_funding_bps` (`0..=10_000`).
    pub min_funding_bps: Option<u32>,
    /// Override `creator_fee_bps` (`0..=10_000`).
    pub creator_fee_bps: Option<u32>,
}

/// Usage metrics tracked per library template.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateUsageStats {
    /// Template identifier.
    pub template_id: Symbol,
    /// Total number of invoices created from this template.
    pub use_count: u64,
    /// Cumulative `total_amount` across all invoices created from this template.
    pub total_volume: i128,
    /// Ledger timestamp of the most recent instantiation (`0` if never used).
    pub last_used_at: u64,
    /// Address of the most recent creator to use this template.
    pub last_used_by: Option<Address>,
}

/// Aggregate summary of the template library and its usage.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarySummary {
    /// Total templates currently registered in the library.
    pub total_templates: u32,
    /// Number of built-in templates.
    pub builtin_count: u32,
    /// Number of custom templates.
    pub custom_count: u32,
    /// Number of currently enabled templates.
    pub enabled_count: u32,
    /// Cumulative instantiations across all templates.
    pub total_uses: u64,
    /// Cumulative invoice volume across all templates.
    pub total_volume: i128,
}

/// Resolved parameters ready to be passed to invoice creation.
#[derive(Clone, Debug)]
pub struct ResolvedTemplateParams {
    pub ratios: Vec<u32>,
    pub amounts: Vec<i128>,
    pub deadline: u64,
    pub allow_early_withdrawal: bool,
    pub min_funding_bps: u32,
    pub creator_fee_bps: u32,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
enum LibraryKey {
    /// Whether built-in templates have been seeded.
    Initialized,
    /// Ordered list of all registered template IDs.
    TemplateIds,
    /// Individual template record by ID.
    Template(Symbol),
    /// Usage statistics per template ID.
    Usage(Symbol),
    /// Per-creator usage count for a template.
    CreatorUsage(Address, Symbol),
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn validate_template_params(
    ratios: &Vec<u32>,
    default_deadline_secs: u64,
    min_funding_bps: u32,
    creator_fee_bps: u32,
) {
    assert!(!ratios.is_empty(), "template ratios must not be empty");
    let sum: u32 = ratios.iter().fold(0u32, |acc, r| {
        assert!(r > 0, "each template ratio must be positive");
        acc.saturating_add(r)
    });
    validation::assert_bps_total(sum).expect("template ratios must sum to 10000 basis points");
    assert!(
        default_deadline_secs > 0,
        "default_deadline_secs must be positive"
    );
    assert!(
        min_funding_bps <= BPS_DENOMINATOR,
        "min_funding_bps must be <= 10000"
    );
    assert!(
        creator_fee_bps <= BPS_DENOMINATOR,
        "creator_fee_bps must be <= 10000"
    );
}

fn load_template_ids(env: &Env) -> Vec<Symbol> {
    env.storage()
        .persistent()
        .get(&LibraryKey::TemplateIds)
        .unwrap_or_else(|| Vec::new(env))
}

fn save_template_ids(env: &Env, ids: &Vec<Symbol>) {
    env.storage()
        .persistent()
        .set(&LibraryKey::TemplateIds, ids);
}

// ---------------------------------------------------------------------------
// Built-in template seeding
// ---------------------------------------------------------------------------

/// Seed the built-in invoice template library.
///
/// Idempotent: if already initialized, returns the current count of built-in
/// templates without overwriting any existing templates or customizations.
pub fn init_library(env: &Env) -> u32 {
    if env
        .storage()
        .persistent()
        .get::<LibraryKey, bool>(&LibraryKey::Initialized)
        .unwrap_or(false)
    {
        return count_builtin_templates(env);
    }

    let now = env.ledger().timestamp();
    let mut ids = load_template_ids(env);

    let builtins: [(
        Symbol,
        &str,
        &str,
        TemplateCategory,
        &[u32],
        u64,
        bool,
        u32,
        u32,
    ); 7] = [
        (
            symbol_short!("equal2"),
            "Equal 2-Way Split",
            "Split an invoice evenly 50/50 between two recipients.",
            TemplateCategory::RevenueShare,
            &[5_000, 5_000],
            604_800, // 7 days
            false,
            0,
            0,
        ),
        (
            symbol_short!("equal3"),
            "Equal 3-Way Split",
            "Split an invoice evenly across three recipients.",
            TemplateCategory::RevenueShare,
            &[3_334, 3_333, 3_333],
            604_800, // 7 days
            false,
            0,
            0,
        ),
        (
            symbol_short!("rev7030"),
            "Creator & Platform 70/30",
            "Standard 70% creator and 30% platform revenue share.",
            TemplateCategory::RevenueShare,
            &[7_000, 3_000],
            1_209_600, // 14 days
            false,
            0,
            0,
        ),
        (
            symbol_short!("pay_team"),
            "Team Payroll Split",
            "3-tier team payroll disbursement (40/35/25) requiring full funding.",
            TemplateCategory::Payroll,
            &[4_000, 3_500, 2_500],
            2_592_000, // 30 days
            false,
            10_000,
            0,
        ),
        (
            symbol_short!("free_ret"),
            "Freelance Retainer",
            "Single-recipient freelance retainer with 50% minimum funding and early withdrawal.",
            TemplateCategory::Freelance,
            &[10_000],
            1_209_600, // 14 days
            true,
            5_000,
            0,
        ),
        (
            symbol_short!("crowd_fn"),
            "Crowdfunding Campaign",
            "Crowdfunding split (85% project / 15% ops) with 75% min funding and 2.5% creator fee.",
            TemplateCategory::Crowdfunding,
            &[8_500, 1_500],
            2_592_000, // 30 days
            true,
            7_500,
            250,
        ),
        (
            symbol_short!("dao_trsy"),
            "DAO Treasury Disbursement",
            "Three-way DAO treasury allocation (50/30/20) requiring 100% funding.",
            TemplateCategory::Treasury,
            &[5_000, 3_000, 2_000],
            604_800, // 7 days
            false,
            10_000,
            0,
        ),
    ];

    let mut seeded = 0u32;
    for (id, name, desc, cat, ratio_slice, deadline_secs, early_wd, min_fund, fee_bps) in builtins {
        if !env
            .storage()
            .persistent()
            .has(&LibraryKey::Template(id.clone()))
        {
            let mut ratios = Vec::new(env);
            for &r in ratio_slice {
                ratios.push_back(r);
            }
            let tmpl = LibraryTemplate {
                id: id.clone(),
                name: String::from_str(env, name),
                description: String::from_str(env, desc),
                category: cat,
                default_ratios: ratios,
                default_deadline_secs: deadline_secs,
                allow_early_withdrawal: early_wd,
                min_funding_bps: min_fund,
                creator_fee_bps: fee_bps,
                is_builtin: true,
                enabled: true,
                version: 1,
                created_by: None,
                created_at: now,
                updated_at: now,
            };
            env.storage()
                .persistent()
                .set(&LibraryKey::Template(id.clone()), &tmpl);
            ids.push_back(id);
            seeded += 1;
        }
    }

    save_template_ids(env, &ids);
    env.storage()
        .persistent()
        .set(&LibraryKey::Initialized, &true);

    seeded
}

fn ensure_initialized(env: &Env) {
    if !env
        .storage()
        .persistent()
        .get::<LibraryKey, bool>(&LibraryKey::Initialized)
        .unwrap_or(false)
    {
        init_library(env);
    }
}

fn count_builtin_templates(env: &Env) -> u32 {
    let ids = load_template_ids(env);
    let mut count = 0u32;
    for id in ids.iter() {
        if let Some(t) = env
            .storage()
            .persistent()
            .get::<LibraryKey, LibraryTemplate>(&LibraryKey::Template(id))
        {
            if t.is_builtin {
                count += 1;
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Template management
// ---------------------------------------------------------------------------

/// Register a new custom template in the library.
pub fn register_template(
    env: &Env,
    creator: Address,
    id: Symbol,
    name: String,
    description: String,
    category: TemplateCategory,
    default_ratios: Vec<u32>,
    default_deadline_secs: u64,
    allow_early_withdrawal: bool,
    min_funding_bps: u32,
    creator_fee_bps: u32,
) -> LibraryTemplate {
    ensure_initialized(env);
    validate_template_params(
        &default_ratios,
        default_deadline_secs,
        min_funding_bps,
        creator_fee_bps,
    );

    assert!(
        !env.storage()
            .persistent()
            .has(&LibraryKey::Template(id.clone())),
        "template id already exists"
    );

    let mut ids = load_template_ids(env);
    assert!(
        ids.len() < MAX_LIBRARY_TEMPLATES,
        "template library capacity reached"
    );

    let now = env.ledger().timestamp();
    let tmpl = LibraryTemplate {
        id: id.clone(),
        name,
        description,
        category,
        default_ratios,
        default_deadline_secs,
        allow_early_withdrawal,
        min_funding_bps,
        creator_fee_bps,
        is_builtin: false,
        enabled: true,
        version: 1,
        created_by: Some(creator),
        created_at: now,
        updated_at: now,
    };

    env.storage()
        .persistent()
        .set(&LibraryKey::Template(id.clone()), &tmpl);
    ids.push_back(id);
    save_template_ids(env, &ids);

    tmpl
}

/// Update an existing library template.
///
/// Built-in templates may only be updated by an admin (`is_admin == true`).
/// Custom templates may be updated by their creator or an admin.
pub fn update_template(
    env: &Env,
    caller: &Address,
    is_admin: bool,
    id: Symbol,
    name: String,
    description: String,
    category: TemplateCategory,
    default_ratios: Vec<u32>,
    default_deadline_secs: u64,
    allow_early_withdrawal: bool,
    min_funding_bps: u32,
    creator_fee_bps: u32,
) -> LibraryTemplate {
    ensure_initialized(env);
    validate_template_params(
        &default_ratios,
        default_deadline_secs,
        min_funding_bps,
        creator_fee_bps,
    );

    let mut tmpl = get_template(env, id.clone());
    assert_can_manage(&tmpl, caller, is_admin);

    tmpl.name = name;
    tmpl.description = description;
    tmpl.category = category;
    tmpl.default_ratios = default_ratios;
    tmpl.default_deadline_secs = default_deadline_secs;
    tmpl.allow_early_withdrawal = allow_early_withdrawal;
    tmpl.min_funding_bps = min_funding_bps;
    tmpl.creator_fee_bps = creator_fee_bps;
    tmpl.version = tmpl.version.saturating_add(1);
    tmpl.updated_at = env.ledger().timestamp();

    env.storage()
        .persistent()
        .set(&LibraryKey::Template(id), &tmpl);

    tmpl
}

/// Enable or disable a library template.
pub fn set_template_enabled(
    env: &Env,
    caller: &Address,
    is_admin: bool,
    id: Symbol,
    enabled: bool,
) -> LibraryTemplate {
    ensure_initialized(env);
    let mut tmpl = get_template(env, id.clone());
    assert_can_manage(&tmpl, caller, is_admin);

    tmpl.enabled = enabled;
    tmpl.updated_at = env.ledger().timestamp();

    env.storage()
        .persistent()
        .set(&LibraryKey::Template(id), &tmpl);

    tmpl
}

/// Remove a custom template from the library.
///
/// Built-in templates cannot be removed (disable them instead).
pub fn remove_template(env: &Env, caller: &Address, is_admin: bool, id: Symbol) {
    ensure_initialized(env);
    let tmpl = get_template(env, id.clone());
    assert!(!tmpl.is_builtin, "cannot remove built-in template");
    assert_can_manage(&tmpl, caller, is_admin);

    env.storage()
        .persistent()
        .remove(&LibraryKey::Template(id.clone()));

    let ids = load_template_ids(env);
    let mut next_ids = Vec::new(env);
    for existing in ids.iter() {
        if existing != id {
            next_ids.push_back(existing);
        }
    }
    save_template_ids(env, &next_ids);
}

fn assert_can_manage(tmpl: &LibraryTemplate, caller: &Address, is_admin: bool) {
    if is_admin {
        return;
    }
    if let Some(ref owner) = tmpl.created_by {
        assert!(owner == caller, "unauthorized to manage template");
    } else {
        panic!("only admin may manage built-in templates");
    }
}

/// Retrieve a single template by ID, panicking if it does not exist.
pub fn get_template(env: &Env, id: Symbol) -> LibraryTemplate {
    ensure_initialized(env);
    env.storage()
        .persistent()
        .get(&LibraryKey::Template(id))
        .expect("library template not found")
}

/// List library templates with optional category and enabled filters.
pub fn list_templates(
    env: &Env,
    category: Option<TemplateCategory>,
    only_enabled: bool,
) -> Vec<LibraryTemplate> {
    ensure_initialized(env);
    let ids = load_template_ids(env);
    let mut result = Vec::new(env);

    for id in ids.iter() {
        if let Some(tmpl) = env
            .storage()
            .persistent()
            .get::<LibraryKey, LibraryTemplate>(&LibraryKey::Template(id))
        {
            if only_enabled && !tmpl.enabled {
                continue;
            }
            if let Some(cat) = category {
                if tmpl.category != cat {
                    continue;
                }
            }
            result.push_back(tmpl);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Customization & resolution
// ---------------------------------------------------------------------------

/// Resolve a library template + optional customization into concrete invoice parameters.
///
/// Validates that the template is enabled, that recipient count matches the
/// effective ratios, and computes exact per-recipient amounts summing to `total_amount`.
pub fn resolve_template_for_invoice(
    env: &Env,
    template_id: &Symbol,
    recipient_count: u32,
    total_amount: i128,
    customization: Option<TemplateCustomization>,
) -> ResolvedTemplateParams {
    ensure_initialized(env);
    assert!(total_amount > 0, "total_amount must be positive");
    assert!(recipient_count > 0, "recipients must not be empty");

    let tmpl = get_template(env, template_id.clone());
    assert!(tmpl.enabled, "library template is disabled");

    let custom = customization.unwrap_or(TemplateCustomization {
        ratios: None,
        deadline: None,
        allow_early_withdrawal: None,
        min_funding_bps: None,
        creator_fee_bps: None,
    });

    let ratios = custom.ratios.unwrap_or(tmpl.default_ratios);
    assert!(
        ratios.len() == recipient_count,
        "recipients and template ratios must have the same length"
    );

    let ratio_sum: u32 = ratios.iter().fold(0u32, |acc, r| {
        assert!(r > 0, "each ratio must be positive");
        acc.saturating_add(r)
    });
    validation::assert_bps_total(ratio_sum).expect("ratios must sum to 10000 basis points");

    let allow_early_withdrawal = custom
        .allow_early_withdrawal
        .unwrap_or(tmpl.allow_early_withdrawal);

    let min_funding_bps = custom.min_funding_bps.unwrap_or(tmpl.min_funding_bps);
    assert!(
        min_funding_bps <= BPS_DENOMINATOR,
        "min_funding_bps must be <= 10000"
    );

    let creator_fee_bps = custom.creator_fee_bps.unwrap_or(tmpl.creator_fee_bps);
    assert!(
        creator_fee_bps <= BPS_DENOMINATOR,
        "creator_fee_bps must be <= 10000"
    );

    let deadline = custom.deadline.unwrap_or_else(|| {
        env.ledger()
            .timestamp()
            .saturating_add(tmpl.default_deadline_secs)
    });

    // Compute per-recipient amounts from basis-point ratios, assigning any
    // integer division remainder to the first recipient so the sum equals `total_amount`.
    let mut amounts = Vec::new(env);
    let mut allocated: i128 = 0;
    let len = ratios.len();
    for i in 0..len {
        let r = ratios.get(i).unwrap();
        let amt = total_amount
            .checked_mul(r as i128)
            .expect("amount overflow")
            / (BPS_DENOMINATOR as i128);
        assert!(
            amt > 0,
            "computed amount for a recipient is zero; increase total_amount"
        );
        allocated += amt;
        amounts.push_back(amt);
    }
    let remainder = total_amount - allocated;
    if remainder > 0 {
        let first = amounts.get(0).unwrap();
        amounts.set(0, first + remainder);
    }

    ResolvedTemplateParams {
        ratios,
        amounts,
        deadline,
        allow_early_withdrawal,
        min_funding_bps,
        creator_fee_bps,
    }
}

// ---------------------------------------------------------------------------
// Usage tracking & analytics
// ---------------------------------------------------------------------------

/// Record a template instantiation for usage analytics.
pub fn record_usage(env: &Env, template_id: &Symbol, creator: &Address, total_amount: i128) {
    let mut stats = get_usage(env, template_id.clone());
    stats.use_count = stats.use_count.saturating_add(1);
    stats.total_volume = stats.total_volume.saturating_add(total_amount);
    stats.last_used_at = env.ledger().timestamp();
    stats.last_used_by = Some(creator.clone());

    env.storage()
        .persistent()
        .set(&LibraryKey::Usage(template_id.clone()), &stats);

    let creator_key = LibraryKey::CreatorUsage(creator.clone(), template_id.clone());
    let prev: u64 = env.storage().persistent().get(&creator_key).unwrap_or(0);
    env.storage()
        .persistent()
        .set(&creator_key, &prev.saturating_add(1));
}

/// Return usage statistics for a given template ID.
pub fn get_usage(env: &Env, template_id: Symbol) -> TemplateUsageStats {
    env.storage()
        .persistent()
        .get(&LibraryKey::Usage(template_id.clone()))
        .unwrap_or(TemplateUsageStats {
            template_id,
            use_count: 0,
            total_volume: 0,
            last_used_at: 0,
            last_used_by: None,
        })
}

/// Return how many times a specific creator has used a given template.
pub fn get_creator_usage(env: &Env, creator: &Address, template_id: Symbol) -> u64 {
    env.storage()
        .persistent()
        .get(&LibraryKey::CreatorUsage(creator.clone(), template_id))
        .unwrap_or(0)
}

/// Return an aggregate summary of the library and its overall usage.
pub fn get_summary(env: &Env) -> LibrarySummary {
    ensure_initialized(env);
    let ids = load_template_ids(env);

    let mut total_templates = 0u32;
    let mut builtin_count = 0u32;
    let mut custom_count = 0u32;
    let mut enabled_count = 0u32;
    let mut total_uses = 0u64;
    let mut total_volume = 0i128;

    for id in ids.iter() {
        if let Some(tmpl) = env
            .storage()
            .persistent()
            .get::<LibraryKey, LibraryTemplate>(&LibraryKey::Template(id.clone()))
        {
            total_templates += 1;
            if tmpl.is_builtin {
                builtin_count += 1;
            } else {
                custom_count += 1;
            }
            if tmpl.enabled {
                enabled_count += 1;
            }
            let usage = get_usage(env, id);
            total_uses = total_uses.saturating_add(usage.use_count);
            total_volume = total_volume.saturating_add(usage.total_volume);
        }
    }

    LibrarySummary {
        total_templates,
        builtin_count,
        custom_count,
        enabled_count,
        total_uses,
        total_volume,
    }
}

/// Return the top `limit` templates sorted by `use_count` descending.
pub fn get_popular_templates(env: &Env, limit: u32) -> Vec<TemplateUsageStats> {
    ensure_initialized(env);
    if limit == 0 {
        return Vec::new(env);
    }

    let ids = load_template_ids(env);
    let mut all = Vec::new(env);
    for id in ids.iter() {
        let stats = get_usage(env, id);
        if stats.use_count > 0 {
            all.push_back(stats);
        }
    }

    // Simple selection sort (bounded by MAX_LIBRARY_TEMPLATES = 100).
    let n = all.len();
    for i in 0..n {
        let mut best = i;
        for j in (i + 1)..n {
            let a = all.get(j).unwrap();
            let b = all.get(best).unwrap();
            if a.use_count > b.use_count
                || (a.use_count == b.use_count && a.total_volume > b.total_volume)
            {
                best = j;
            }
        }
        if best != i {
            let tmp = all.get(i).unwrap();
            let best_val = all.get(best).unwrap();
            all.set(i, best_val);
            all.set(best, tmp);
        }
    }

    let take = if limit < n { limit } else { n };
    let mut top = Vec::new(env);
    for i in 0..take {
        top.push_back(all.get(i).unwrap());
    }
    top
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    #[test]
    fn test_init_library_seeds_builtins_idempotently() {
        let env = Env::default();
        let seeded = init_library(&env);
        assert_eq!(seeded, 7);

        // Second call is idempotent and does not duplicate.
        let seeded_again = init_library(&env);
        assert_eq!(seeded_again, 7);

        let all = list_templates(&env, None, false);
        assert_eq!(all.len(), 7);

        let equal2 = get_template(&env, symbol_short!("equal2"));
        assert!(equal2.is_builtin);
        assert!(equal2.enabled);
        assert_eq!(equal2.default_ratios.len(), 2);
        assert_eq!(equal2.default_ratios.get(0).unwrap(), 5_000);
        assert_eq!(equal2.default_ratios.get(1).unwrap(), 5_000);
    }

    #[test]
    fn test_register_update_toggle_and_remove_custom_template() {
        let env = Env::default();
        let creator = Address::generate(&env);
        let id = symbol_short!("my_split");

        let mut ratios = Vec::new(&env);
        ratios.push_back(6_000);
        ratios.push_back(4_000);

        let created = register_template(
            &env,
            creator.clone(),
            id.clone(),
            String::from_str(&env, "My Custom Split"),
            String::from_str(&env, "60/40 custom split"),
            TemplateCategory::Custom,
            ratios,
            86_400,
            true,
            2_500,
            100,
        );
        assert!(!created.is_builtin);
        assert_eq!(created.version, 1);
        assert!(created.enabled);

        // Update by creator increments version.
        let mut new_ratios = Vec::new(&env);
        new_ratios.push_back(8_000);
        new_ratios.push_back(2_000);
        let updated = update_template(
            &env,
            &creator,
            false,
            id.clone(),
            String::from_str(&env, "My Updated Split"),
            String::from_str(&env, "80/20 custom split"),
            TemplateCategory::RevenueShare,
            new_ratios,
            172_800,
            false,
            5_000,
            200,
        );
        assert_eq!(updated.version, 2);
        assert_eq!(updated.default_ratios.get(0).unwrap(), 8_000);

        // Disable and verify filtering.
        let disabled = set_template_enabled(&env, &creator, false, id.clone(), false);
        assert!(!disabled.enabled);

        let enabled_only = list_templates(&env, None, true);
        assert_eq!(enabled_only.len(), 7); // only built-ins enabled

        // Remove custom template.
        remove_template(&env, &creator, false, id.clone());
        let all = list_templates(&env, None, false);
        assert_eq!(all.len(), 7);
    }

    #[test]
    fn test_resolve_with_customization_and_remainder() {
        let env = Env::default();
        let id = symbol_short!("equal3");

        // Default resolution with 100 stroops -> 33 + 33 + 33 + 1 remainder = 100.
        let resolved = resolve_template_for_invoice(&env, &id, 3, 100, None);
        let sum: i128 = resolved.amounts.iter().sum();
        assert_eq!(sum, 100);
        assert_eq!(resolved.amounts.len(), 3);

        // Customized overrides.
        let mut custom_ratios = Vec::new(&env);
        custom_ratios.push_back(5_000);
        custom_ratios.push_back(3_000);
        custom_ratios.push_back(2_000);

        let custom = TemplateCustomization {
            ratios: Some(custom_ratios),
            deadline: Some(999_999),
            allow_early_withdrawal: Some(true),
            min_funding_bps: Some(8_000),
            creator_fee_bps: Some(150),
        };
        let resolved_custom = resolve_template_for_invoice(&env, &id, 3, 10_000, Some(custom));
        assert_eq!(resolved_custom.amounts.get(0).unwrap(), 5_000);
        assert_eq!(resolved_custom.amounts.get(1).unwrap(), 3_000);
        assert_eq!(resolved_custom.amounts.get(2).unwrap(), 2_000);
        assert_eq!(resolved_custom.deadline, 999_999);
        assert!(resolved_custom.allow_early_withdrawal);
        assert_eq!(resolved_custom.min_funding_bps, 8_000);
        assert_eq!(resolved_custom.creator_fee_bps, 150);
    }

    #[test]
    fn test_usage_tracking_and_popularity() {
        let env = Env::default();
        init_library(&env);

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);

        record_usage(&env, &symbol_short!("equal2"), &alice, 1_000);
        record_usage(&env, &symbol_short!("equal2"), &bob, 2_500);
        record_usage(&env, &symbol_short!("rev7030"), &alice, 5_000);

        let eq2_stats = get_usage(&env, symbol_short!("equal2"));
        assert_eq!(eq2_stats.use_count, 2);
        assert_eq!(eq2_stats.total_volume, 3_500);
        assert_eq!(eq2_stats.last_used_by, Some(bob.clone()));

        assert_eq!(get_creator_usage(&env, &alice, symbol_short!("equal2")), 1);
        assert_eq!(get_creator_usage(&env, &bob, symbol_short!("equal2")), 1);

        let popular = get_popular_templates(&env, 2);
        assert_eq!(popular.len(), 2);
        assert_eq!(popular.get(0).unwrap().template_id, symbol_short!("equal2"));
        assert_eq!(popular.get(1).unwrap().template_id, symbol_short!("rev7030"));

        let summary = get_summary(&env);
        assert_eq!(summary.total_templates, 7);
        assert_eq!(summary.builtin_count, 7);
        assert_eq!(summary.custom_count, 0);
        assert_eq!(summary.total_uses, 3);
        assert_eq!(summary.total_volume, 8_500);
    }
}
