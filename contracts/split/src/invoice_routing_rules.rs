//! Issue #1167: condition-based payment routing (first slice).
//!
//! Pure helpers: given an ordered rule list, an amount and a payer, pick the
//! destination of the matching rule with the lowest `priority` value (first
//! rule wins ties). Not covered: contract entry points, persistent rule
//! storage, persisted decision history and events.

use soroban_sdk::Address;

/// Maximum number of rules evaluated per routing call.
pub const MAX_ROUTING_RULES: usize = 50;

/// Errors for routing helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingError {
    /// Amount must be positive.
    InvalidAmount,
    /// Too many rules supplied.
    TooManyRules,
    /// No rule matched.
    NoMatch,
}

/// Condition under which a rule applies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoutingCondition {
    /// Always matches.
    Always,
    /// Matches when amount >= value.
    AmountAtLeast(i128),
    /// Matches when amount < value.
    AmountBelow(i128),
    /// Matches when the payer equals the address.
    PayerIs(Address),
}

/// A single routing rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingRule {
    /// Condition to evaluate.
    pub condition: RoutingCondition,
    /// Destination when matched.
    pub destination: Address,
    /// Lower value is evaluated as higher priority.
    pub priority: u32,
}

/// Record of a routing decision (for tracking by the caller).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingDecision {
    /// Index of the winning rule in the input slice.
    pub rule_index: u32,
    /// Chosen destination.
    pub destination: Address,
}

/// Returns true when `condition` holds for the amount and payer.
pub fn condition_matches(condition: &RoutingCondition, amount: i128, payer: &Address) -> bool {
    match condition {
        RoutingCondition::Always => true,
        RoutingCondition::AmountAtLeast(v) => amount >= *v,
        RoutingCondition::AmountBelow(v) => amount < *v,
        RoutingCondition::PayerIs(a) => a == payer,
    }
}

/// Selects the destination for a payment from `rules`.
pub fn route_payment(
    rules: &[RoutingRule],
    amount: i128,
    payer: &Address,
) -> Result<RoutingDecision, RoutingError> {
    if amount <= 0 {
        return Err(RoutingError::InvalidAmount);
    }
    if rules.len() > MAX_ROUTING_RULES {
        return Err(RoutingError::TooManyRules);
    }
    let mut best: Option<(usize, u32)> = None;
    for (i, rule) in rules.iter().enumerate() {
        if !condition_matches(&rule.condition, amount, payer) {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, p)) => rule.priority < p,
        };
        if better {
            best = Some((i, rule.priority));
        }
    }
    match best {
        Some((i, _)) => Ok(RoutingDecision {
            rule_index: i as u32,
            destination: rules[i].destination.clone(),
        }),
        None => Err(RoutingError::NoMatch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    #[test]
    fn priority_and_conditions() {
        let env = Env::default();
        let payer = Address::generate(&env);
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let rules = [
            RoutingRule { condition: RoutingCondition::Always, destination: a.clone(), priority: 5 },
            RoutingRule { condition: RoutingCondition::AmountAtLeast(100), destination: b.clone(), priority: 1 },
        ];
        assert_eq!(route_payment(&rules, 100, &payer).unwrap().destination, b);
        let d = route_payment(&rules, 10, &payer).unwrap();
        assert_eq!(d.destination, a);
        assert_eq!(d.rule_index, 0);
    }

    #[test]
    fn errors() {
        let env = Env::default();
        let payer = Address::generate(&env);
        assert_eq!(route_payment(&[], 0, &payer), Err(RoutingError::InvalidAmount));
        assert_eq!(route_payment(&[], 5, &payer), Err(RoutingError::NoMatch));
    }
}
