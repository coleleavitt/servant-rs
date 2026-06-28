//! AI Copilot — proposal generator (Phase 7-8, AIProposals parity). Composes the existing pure
//! tools into a client-ready PROPOSAL for an account: the recommended model, the rebalance trades
//! to reach it, the resulting drift reduction, and the realized-gain tax impact of the sells.
//! Deterministic + testable; the copilot narrates around this structured object.

use serde::{Deserialize, Serialize};

use crate::domain::{ModelHolding, Quotes, Trade, TradeAction, ValuedPortfolio, rebalance_trades};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub model_name: String,
    pub trades: Vec<Trade>,
    pub buy_count: usize,
    pub sell_count: usize,
    pub turnover: f64,         // total $ traded
    pub pre_active_share: f64, // % of the basket away from target before trading
    pub post_active_share: f64,
    pub estimated_taxable_gain: f64, // realized gain on the SELLs (taxable accounts)
}

/// Active share = ½·Σ|actual% − target%| over the union of tickers (0 = on model, 100 = disjoint).
fn active_share(valued: &ValuedPortfolio, model: &[ModelHolding]) -> f64 {
    use std::collections::BTreeMap;
    let mut tgt: BTreeMap<String, f64> = BTreeMap::new();
    for h in model {
        *tgt.entry(h.ticker.to_uppercase()).or_insert(0.0) += h.target_weight;
    }
    let mut act: BTreeMap<String, f64> = BTreeMap::new();
    for p in &valued.positions {
        if let Some(w) = p.weight {
            *act.entry(p.ticker.to_uppercase()).or_insert(0.0) += w;
        }
    }
    let mut tickers: Vec<String> = tgt.keys().chain(act.keys()).cloned().collect();
    tickers.sort();
    tickers.dedup();
    let sum: f64 = tickers
        .iter()
        .map(|t| (act.get(t).copied().unwrap_or(0.0) - tgt.get(t).copied().unwrap_or(0.0)).abs())
        .sum();
    sum / 2.0
}

/// Build a proposal: rebalance `valued` to `model`, then summarize. `cost_basis` is per-ticker
/// average cost (for the tax estimate); a SELL realizes (price − cost)·shares of gain.
pub fn build_proposal(
    model_name: &str,
    valued: &ValuedPortfolio,
    model: &[ModelHolding],
    quotes: &Quotes,
    taxable: bool,
) -> Proposal {
    let trades = rebalance_trades(valued, model, quotes, 1.0);
    let pre = active_share(valued, model);

    // post-trade active share: apply the trades to the weights and recompute against the model.
    // We approximate by using each trade's post_trade_weight (already vs total) for traded names.
    use std::collections::BTreeMap;
    let mut post_w: BTreeMap<String, f64> = valued
        .positions
        .iter()
        .filter_map(|p| p.weight.map(|w| (p.ticker.to_uppercase(), w)))
        .collect();
    for t in &trades {
        post_w.insert(t.ticker.to_uppercase(), t.post_trade_weight);
    }
    let mut tgt: BTreeMap<String, f64> = BTreeMap::new();
    for h in model {
        *tgt.entry(h.ticker.to_uppercase()).or_insert(0.0) += h.target_weight;
    }
    let mut tickers: Vec<String> = tgt.keys().chain(post_w.keys()).cloned().collect();
    tickers.sort();
    tickers.dedup();
    let post: f64 = tickers
        .iter()
        .map(|t| (post_w.get(t).copied().unwrap_or(0.0) - tgt.get(t).copied().unwrap_or(0.0)).abs())
        .sum::<f64>()
        / 2.0;

    let cost_by: BTreeMap<String, f64> = valued
        .positions
        .iter()
        .map(|p| (p.ticker.to_uppercase(), p.cost_basis))
        .collect();
    let mut taxable_gain = 0.0;
    let mut turnover = 0.0;
    let (mut buys, mut sells) = (0, 0);
    for t in &trades {
        turnover += t.amount;
        match t.action {
            TradeAction::Buy => buys += 1,
            TradeAction::Sell => {
                sells += 1;
                if taxable {
                    let cost = cost_by
                        .get(&t.ticker.to_uppercase())
                        .copied()
                        .unwrap_or(0.0);
                    let price = quotes.get(&t.ticker.to_uppercase()).copied().unwrap_or(0.0);
                    taxable_gain += (price - cost) * t.shares;
                }
            }
        }
    }

    Proposal {
        model_name: model_name.to_string(),
        trades,
        buy_count: buys,
        sell_count: sells,
        turnover,
        pre_active_share: pre,
        post_active_share: post,
        estimated_taxable_gain: taxable_gain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Position, value_positions};

    fn q() -> Quotes {
        [("AAPL".into(), 100.0), ("MSFT".into(), 100.0)]
            .into_iter()
            .collect()
    }

    #[test]
    fn proposal_reduces_active_share() {
        // 80/20 actual → 60/40 model
        let pos = vec![
            Position {
                ticker: "AAPL".into(),
                shares: 80.0,
                cost_basis: 50.0,
            },
            Position {
                ticker: "MSFT".into(),
                shares: 20.0,
                cost_basis: 100.0,
            },
        ];
        let valued = value_positions(&pos, &q(), 0.0);
        let model = vec![
            ModelHolding {
                ticker: "AAPL".into(),
                target_weight: 60.0,
            },
            ModelHolding {
                ticker: "MSFT".into(),
                target_weight: 40.0,
            },
        ];
        let p = build_proposal("60/40", &valued, &model, &q(), true);
        assert!(p.pre_active_share > p.post_active_share); // moves toward the model
        assert_eq!(p.sell_count, 1); // sell AAPL
        assert_eq!(p.buy_count, 1); // buy MSFT
        // sells 20 AAPL @100 cost 50 → realized gain 1000
        assert_eq!(p.estimated_taxable_gain, 1000.0);
    }

    #[test]
    fn tax_advantaged_has_no_taxable_gain() {
        let pos = vec![Position {
            ticker: "AAPL".into(),
            shares: 80.0,
            cost_basis: 50.0,
        }];
        let valued = value_positions(&pos, &q(), 0.0);
        let model = vec![ModelHolding {
            ticker: "AAPL".into(),
            target_weight: 50.0,
        }];
        let p = build_proposal("half", &valued, &model, &q(), false);
        assert_eq!(p.estimated_taxable_gain, 0.0);
    }
}
