//! Agentic layer (Phase 7-8). The first agent is the **tax-loss-harvesting (TLH) agent** —
//! IDEA.md §6.4: TLH fused into the loop, not bolted on. It is DETERMINISTIC and pure: given an
//! account's valued holdings, it proposes harvest sells for positions sitting at a loss past a
//! threshold, and (critically) every proposed action is checked against the same risk gate the
//! paper engine uses. No LLM can widen the gate. The model "proposes", the gate "disposes".
//!
//! This is the safe core an LLM agent calls: the LLM picks intent, this module enforces rules.

use serde::{Deserialize, Serialize};

use crate::domain::{AccountType, TradeAction, ValuedPortfolio};
use crate::paper::{RiskLimits, risk_check};

/// A proposed harvest action with the rationale + whether it passes the risk gate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarvestProposal {
    pub ticker: String,
    pub shares: f64,
    pub price: f64,
    pub unrealized_loss: f64, // negative dollars
    pub loss_pct: f64,        // negative percent
    pub admissible: bool,     // passes the deterministic risk gate
    pub rationale: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarvestPlan {
    pub account_type: String,
    pub eligible: bool, // tax-advantaged accounts have no taxable losses to harvest
    pub proposals: Vec<HarvestProposal>,
    pub total_harvestable_loss: f64,
}

/// Propose tax-loss-harvest sells. Only taxable accounts are eligible (no benefit in an IRA).
/// `min_loss_pct` is the loss threshold (e.g. -5.0 = at least 5% down) before harvesting.
pub fn propose_harvest(
    account_type: AccountType,
    valued: &ValuedPortfolio,
    min_loss_pct: f64,
    limits: &RiskLimits,
) -> HarvestPlan {
    if account_type.is_tax_advantaged() {
        return HarvestPlan {
            account_type: account_type.as_str().to_string(),
            eligible: false,
            proposals: vec![],
            total_harvestable_loss: 0.0,
        };
    }
    let account_value = valued.total_value;
    let mut proposals = vec![];
    let mut total_loss = 0.0;
    for p in &valued.positions {
        let (price, gain) = match (p.price, p.gain) {
            (Some(pr), Some(g)) => (pr, g),
            _ => continue,
        };
        if p.shares <= 0.0 || gain >= 0.0 {
            continue;
        }
        let cost = p.cost_basis * p.shares;
        let loss_pct = if cost > 0.0 { gain / cost * 100.0 } else { 0.0 };
        if loss_pct > min_loss_pct {
            continue; // not down enough (min_loss_pct is negative)
        }
        // a full-position harvest sell — gate it like any real order.
        let admissible = risk_check(
            limits,
            TradeAction::Sell,
            p.shares,
            price,
            account_value,
            0.0, // cash not relevant to a sell
            p.shares,
        )
        .is_ok();
        total_loss += gain;
        proposals.push(HarvestProposal {
            ticker: p.ticker.clone(),
            shares: p.shares,
            price,
            unrealized_loss: gain,
            loss_pct,
            admissible,
            rationale: format!(
                "{} down {:.1}% ({:.0} loss) — harvest to realize the tax loss",
                p.ticker, loss_pct, gain
            ),
        });
    }
    // biggest losses first
    proposals.sort_by(|a, b| {
        a.unrealized_loss
            .partial_cmp(&b.unrealized_loss)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    HarvestPlan {
        account_type: account_type.as_str().to_string(),
        eligible: true,
        proposals,
        total_harvestable_loss: total_loss,
    }
}

// ── AX-AI advisor agent: a deterministic review loop over an account ──────────

/// One recommended action the agent surfaces, with priority + risk-gate status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recommendation {
    pub kind: String, // "rebalance" | "harvest" | "concentration" | "cash-drag"
    pub priority: u8, // 1 (highest) .. 5
    pub detail: String,
    pub admissible: bool, // a concrete action exists that clears the risk gate
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdvisorReview {
    pub account_type: String,
    pub recommendations: Vec<Recommendation>,
}

/// Inputs the agent reasons over (gathered by the caller from the DB/tools).
pub struct ReviewInputs<'a> {
    pub account_type: AccountType,
    pub valued: &'a ValuedPortfolio,
    pub harvest: &'a HarvestPlan,
    /// max single-position weight (%) before flagging concentration risk.
    pub concentration_pct: f64,
    /// cash weight (%) above which idle-cash drag is flagged.
    pub cash_drag_pct: f64,
    /// portfolio cash as a fraction already included in valued.total_value.
    pub cash: f64,
}

/// Run the deterministic advisor review: produce a prioritized recommendation list. The agent
/// only RECOMMENDS — each item notes whether a gate-clearing action exists, but nothing executes
/// here. An LLM narrates around this; the priorities + admissibility are pure.
pub fn advisor_review(inp: &ReviewInputs) -> AdvisorReview {
    let mut recs = vec![];

    // 1) harvestable losses (taxable only) — high priority, admissibility from the plan.
    if inp.harvest.eligible && !inp.harvest.proposals.is_empty() {
        let admissible = inp.harvest.proposals.iter().any(|p| p.admissible);
        recs.push(Recommendation {
            kind: "harvest".into(),
            priority: 2,
            detail: format!(
                "{} position(s) at a loss — {:.0} harvestable",
                inp.harvest.proposals.len(),
                inp.harvest.total_harvestable_loss
            ),
            admissible,
        });
    }

    // 2) concentration risk — any single holding above the threshold.
    if let Some(top) = inp
        .valued
        .positions
        .iter()
        .filter_map(|p| p.weight.map(|w| (p.ticker.clone(), w)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    {
        if top.1 > inp.concentration_pct {
            recs.push(Recommendation {
                kind: "concentration".into(),
                priority: 1,
                detail: format!(
                    "{} is {:.0}% of the account — above the {:.0}% concentration limit",
                    top.0, top.1, inp.concentration_pct
                ),
                admissible: true, // trimming is always gate-admissible (sell of held shares)
            });
        }
    }

    // 3) cash drag — idle cash above threshold.
    if inp.valued.total_value > 0.0 {
        let cash_w = inp.cash / inp.valued.total_value * 100.0;
        if cash_w > inp.cash_drag_pct {
            recs.push(Recommendation {
                kind: "cash-drag".into(),
                priority: 3,
                detail: format!(
                    "{:.0}% idle cash — above the {:.0}% target; consider deploying",
                    cash_w, inp.cash_drag_pct
                ),
                admissible: true,
            });
        }
    }

    recs.sort_by_key(|r| r.priority);
    AdvisorReview {
        account_type: inp.account_type.as_str().to_string(),
        recommendations: recs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Position, Quotes, value_positions};

    fn valued(pos: &[Position], q: &Quotes) -> ValuedPortfolio {
        value_positions(pos, q, 0.0)
    }

    #[test]
    fn ira_is_ineligible() {
        let pos = vec![Position {
            ticker: "AAPL".into(),
            shares: 10.0,
            cost_basis: 200.0,
        }];
        let q: Quotes = [("AAPL".into(), 100.0)].into_iter().collect();
        let plan = propose_harvest(
            AccountType::RothIRA,
            &valued(&pos, &q),
            -5.0,
            &RiskLimits::default(),
        );
        assert!(!plan.eligible);
        assert!(plan.proposals.is_empty());
    }

    #[test]
    fn taxable_harvests_losers_past_threshold() {
        // AAPL 10 @ cost 200, now 100 → 50% loss (harvest). MSFT 10 @ 100, now 110 → gain (skip).
        let pos = vec![
            Position {
                ticker: "AAPL".into(),
                shares: 10.0,
                cost_basis: 200.0,
            },
            Position {
                ticker: "MSFT".into(),
                shares: 10.0,
                cost_basis: 100.0,
            },
        ];
        let q: Quotes = [("AAPL".into(), 100.0), ("MSFT".into(), 110.0)]
            .into_iter()
            .collect();
        // wide gate so the harvest sell is admissible
        let limits = RiskLimits {
            max_order_frac: 1.0,
            allow_short: false,
            max_cash_use_frac: 1.0,
        };
        let plan = propose_harvest(AccountType::Individual, &valued(&pos, &q), -5.0, &limits);
        assert!(plan.eligible);
        assert_eq!(plan.proposals.len(), 1);
        let p = &plan.proposals[0];
        assert_eq!(p.ticker, "AAPL");
        assert_eq!(p.unrealized_loss, -1000.0); // (100-200)*10
        assert!(p.admissible);
        assert_eq!(plan.total_harvestable_loss, -1000.0);
    }

    #[test]
    fn small_loss_below_threshold_is_skipped() {
        // AAPL down only 2%, threshold -5% → skip
        let pos = vec![Position {
            ticker: "AAPL".into(),
            shares: 10.0,
            cost_basis: 100.0,
        }];
        let q: Quotes = [("AAPL".into(), 98.0)].into_iter().collect();
        let plan = propose_harvest(
            AccountType::Individual,
            &valued(&pos, &q),
            -5.0,
            &RiskLimits::default(),
        );
        assert!(plan.proposals.is_empty());
    }

    #[test]
    fn risk_gate_marks_oversized_harvest_inadmissible() {
        // huge single position vs default 25% gate → proposed but admissible=false
        let pos = vec![Position {
            ticker: "AAPL".into(),
            shares: 100.0,
            cost_basis: 200.0,
        }];
        let q: Quotes = [("AAPL".into(), 100.0)].into_iter().collect();
        let plan = propose_harvest(
            AccountType::Individual,
            &valued(&pos, &q),
            -5.0,
            &RiskLimits::default(),
        );
        assert_eq!(plan.proposals.len(), 1);
        assert!(!plan.proposals[0].admissible); // gate caught the oversized sell
    }
    #[test]
    fn advisor_flags_concentration_and_harvest() {
        // AAPL 90% (loss), MSFT 10% — taxable
        let pos = vec![
            Position {
                ticker: "AAPL".into(),
                shares: 90.0,
                cost_basis: 200.0,
            },
            Position {
                ticker: "MSFT".into(),
                shares: 10.0,
                cost_basis: 50.0,
            },
        ];
        let q: Quotes = [("AAPL".into(), 100.0), ("MSFT".into(), 100.0)]
            .into_iter()
            .collect();
        let valued = value_positions(&pos, &q, 0.0);
        let limits = RiskLimits {
            max_order_frac: 1.0,
            allow_short: false,
            max_cash_use_frac: 1.0,
        };
        let harvest = propose_harvest(AccountType::Individual, &valued, -5.0, &limits);
        let inp = ReviewInputs {
            account_type: AccountType::Individual,
            valued: &valued,
            harvest: &harvest,
            concentration_pct: 40.0,
            cash_drag_pct: 10.0,
            cash: 0.0,
        };
        let r = advisor_review(&inp);
        // concentration is priority 1 (first), harvest priority 2
        assert_eq!(r.recommendations[0].kind, "concentration");
        assert!(r.recommendations.iter().any(|x| x.kind == "harvest"));
    }

    #[test]
    fn advisor_flags_cash_drag() {
        let pos = vec![Position {
            ticker: "AAPL".into(),
            shares: 10.0,
            cost_basis: 100.0,
        }];
        let q: Quotes = [("AAPL".into(), 100.0)].into_iter().collect();
        // 1000 in AAPL + 4000 cash = 5000 → 80% cash
        let valued = value_positions(&pos, &q, 4000.0);
        let harvest = propose_harvest(AccountType::RothIRA, &valued, -5.0, &RiskLimits::default());
        let inp = ReviewInputs {
            account_type: AccountType::RothIRA,
            valued: &valued,
            harvest: &harvest,
            concentration_pct: 90.0,
            cash_drag_pct: 10.0,
            cash: 4000.0,
        };
        let r = advisor_review(&inp);
        assert!(r.recommendations.iter().any(|x| x.kind == "cash-drag"));
    }
}
