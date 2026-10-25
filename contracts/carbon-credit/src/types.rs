use soroban_sdk::{Symbol, contracttype, i128};

#[contracttype]
#[derive(Clone, Debug)]
pub struct CarbonRecord {
    pub invoice_id: Symbol,
    pub credits: i128,
    pub source: Symbol,
    pub timestamp: u64,
    pub verified: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct WalletCarbonBalance {
    pub wallet: soroban_sdk::Address,
    pub total_credits: i128,
    pub redeemed_credits: i128,
    pub available_credits: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CarbonSummary {
    pub total_registered: i128,
    pub total_verified: i128,
    pub total_linked: i128,
    pub total_redeemed: i128,
}
