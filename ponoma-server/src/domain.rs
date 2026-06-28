//! Domain types + pure analytics for ponoma's book of record. Mirrors the TS pure-math core
//! (portfolio/rebalance/tolerance) in Rust so the backend computes identically. No I/O here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Orion-parity account/registration types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountType {
    Individual,
    JointJTWROS,
    JointTenantsInCommon,
    TOD,
    RevocableTrust,
    IrrevocableTrust,
    TraditionalIRA,
    RothIRA,
    RolloverIRA,
    SEPIRA,
    SIMPLEIRA,
    FourOhOneK,
    FourOhThreeB,
    FivedTwentyNine,
    HSA,
    UTMA,
    UGMA,
    Corporate,
    Partnership,
    LLC,
}

impl AccountType {
    /// Is this a tax-advantaged registration (no taxable events on internal trades)?
    pub fn is_tax_advantaged(self) -> bool {
        matches!(
            self,
            AccountType::TraditionalIRA
                | AccountType::RothIRA
                | AccountType::RolloverIRA
                | AccountType::SEPIRA
                | AccountType::SIMPLEIRA
                | AccountType::FourOhOneK
                | AccountType::FourOhThreeB
                | AccountType::FivedTwentyNine
                | AccountType::HSA
        )
    }
    pub fn as_str(self) -> &'static str {
        match self {
            AccountType::Individual => "Individual",
            AccountType::JointJTWROS => "Joint JTWROS",
            AccountType::JointTenantsInCommon => "Joint Tenants in Common",
            AccountType::TOD => "TOD",
            AccountType::RevocableTrust => "Revocable Trust",
            AccountType::IrrevocableTrust => "Irrevocable Trust",
            AccountType::TraditionalIRA => "Traditional IRA",
            AccountType::RothIRA => "Roth IRA",
            AccountType::RolloverIRA => "Rollover IRA",
            AccountType::SEPIRA => "SEP IRA",
            AccountType::SIMPLEIRA => "SIMPLE IRA",
            AccountType::FourOhOneK => "401(k)",
            AccountType::FourOhThreeB => "403(b)",
            AccountType::FivedTwentyNine => "529",
            AccountType::HSA => "HSA",
            AccountType::UTMA => "UTMA",
            AccountType::UGMA => "UGMA",
            AccountType::Corporate => "Corporate",
            AccountType::Partnership => "Partnership",
            AccountType::LLC => "LLC",
        }
    }

    /// Parse the display string back to the enum; unknown → Individual (taxable default).
    pub fn from_str_name(s: &str) -> Self {
        match s {
            "Joint JTWROS" => AccountType::JointJTWROS,
            "Joint Tenants in Common" => AccountType::JointTenantsInCommon,
            "TOD" => AccountType::TOD,
            "Revocable Trust" => AccountType::RevocableTrust,
            "Irrevocable Trust" => AccountType::IrrevocableTrust,
            "Traditional IRA" => AccountType::TraditionalIRA,
            "Roth IRA" => AccountType::RothIRA,
            "Rollover IRA" => AccountType::RolloverIRA,
            "SEP IRA" => AccountType::SEPIRA,
            "SIMPLE IRA" => AccountType::SIMPLEIRA,
            "401(k)" => AccountType::FourOhOneK,
            "403(b)" => AccountType::FourOhThreeB,
            "529" => AccountType::FivedTwentyNine,
            "HSA" => AccountType::HSA,
            "UTMA" => AccountType::UTMA,
            "UGMA" => AccountType::UGMA,
            "Corporate" => AccountType::Corporate,
            "Partnership" => AccountType::Partnership,
            "LLC" => AccountType::LLC,
            _ => AccountType::Individual,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub ticker: String,
    pub shares: f64,
    pub cost_basis: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelHolding {
    pub ticker: String,
    pub target_weight: f64, // percent
}

/// A {ticker -> price} map.
pub type Quotes = BTreeMap<String, f64>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValuedPosition {
    pub ticker: String,
    pub shares: f64,
    pub cost_basis: f64,
    pub price: Option<f64>,
    pub market_value: Option<f64>,
    pub weight: Option<f64>, // percent of portfolio
    pub gain: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValuedPortfolio {
    pub positions: Vec<ValuedPosition>,
    pub total_value: f64,
    pub total_cost: f64,
    pub total_gain: f64,
}

/// Value a set of positions from a quote map. Missing prices -> None (not zero).
pub fn value_positions(positions: &[Position], quotes: &Quotes, cash: f64) -> ValuedPortfolio {
    let priced: Vec<(f64, Option<f64>, f64, Option<f64>)> = positions
        .iter()
        .map(|p| {
            let price = quotes.get(&p.ticker.to_uppercase()).copied();
            let mv = price.map(|pr| pr * p.shares);
            let cost = p.cost_basis * p.shares;
            let gain = mv.map(|m| m - cost);
            (cost, mv, p.shares, gain)
        })
        .collect();
    let total_value: f64 = priced
        .iter()
        .map(|(_, mv, _, _)| mv.unwrap_or(0.0))
        .sum::<f64>()
        + cash;
    let total_cost: f64 = priced
        .iter()
        .map(|(cost, mv, _, _)| if mv.is_some() { *cost } else { 0.0 })
        .sum();
    let total_gain: f64 = priced.iter().map(|(_, _, _, g)| g.unwrap_or(0.0)).sum();
    let vpos = positions
        .iter()
        .zip(priced.iter())
        .map(|(p, (_, mv, _, gain))| ValuedPosition {
            ticker: p.ticker.clone(),
            shares: p.shares,
            cost_basis: p.cost_basis,
            price: quotes.get(&p.ticker.to_uppercase()).copied(),
            market_value: *mv,
            weight: mv.map(|m| {
                if total_value > 0.0 {
                    m / total_value * 100.0
                } else {
                    0.0
                }
            }),
            gain: *gain,
        })
        .collect();
    ValuedPortfolio {
        positions: vpos,
        total_value,
        total_cost,
        total_gain,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeAction {
    Buy,
    Sell,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trade {
    pub ticker: String,
    pub action: TradeAction,
    pub shares: f64,
    pub amount: f64,
    pub current_weight: f64,
    pub target_weight: f64,
    pub post_trade_weight: f64,
}

/// Rebalance positions toward a model's target weights (whole shares, min-trade dust filter).
/// Mirrors the TS `rebalanceTrades`.
pub fn rebalance_trades(
    valued: &ValuedPortfolio,
    model: &[ModelHolding],
    quotes: &Quotes,
    min_trade_amount: f64,
) -> Vec<Trade> {
    let total = valued.total_value;
    if total <= 0.0 {
        return vec![];
    }
    let cur_val: BTreeMap<String, f64> = valued
        .positions
        .iter()
        .map(|p| (p.ticker.to_uppercase(), p.market_value.unwrap_or(0.0)))
        .collect();
    let cur_wt: BTreeMap<String, f64> = valued
        .positions
        .iter()
        .map(|p| (p.ticker.to_uppercase(), p.weight.unwrap_or(0.0)))
        .collect();
    let target: BTreeMap<String, f64> = model
        .iter()
        .map(|h| (h.ticker.to_uppercase(), h.target_weight))
        .collect();

    let mut tickers: Vec<String> = cur_val.keys().chain(target.keys()).cloned().collect();
    tickers.sort();
    tickers.dedup();

    let mut trades = vec![];
    for t in tickers {
        let price = match quotes.get(&t) {
            Some(p) if *p > 0.0 => *p,
            _ => continue,
        };
        let tgt = target.get(&t).copied().unwrap_or(0.0);
        let cur_w = cur_wt.get(&t).copied().unwrap_or(0.0);
        let target_val = tgt / 100.0 * total;
        let cv = cur_val.get(&t).copied().unwrap_or(0.0);
        let delta = target_val - cv;
        if delta.abs() < min_trade_amount {
            continue;
        }
        let shares = (delta.abs() / price).floor();
        if shares <= 0.0 {
            continue;
        }
        let traded = shares * price;
        let post = if delta > 0.0 {
            cv + traded
        } else {
            cv - traded
        };
        trades.push(Trade {
            ticker: t,
            action: if delta > 0.0 {
                TradeAction::Buy
            } else {
                TradeAction::Sell
            },
            shares,
            amount: traded,
            current_weight: cur_w,
            target_weight: tgt,
            post_trade_weight: post / total * 100.0,
        });
    }
    trades.sort_by(|a, b| {
        b.amount
            .partial_cmp(&a.amount)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    trades
}

/// A ledger transaction (for performance math). `amount` is the signed cash impact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LedgerTxn {
    pub kind: String,
    pub ticker: String,
    pub shares: f64,
    pub price: f64,
    pub amount: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Performance {
    pub realized_pl: f64,
    pub unrealized_pl: f64,
    pub dividends: f64,
    pub fees: f64,
    pub total_pl: f64,
}

/// Realized (average-cost on SELLs + dividends - fees) + unrealized (current holdings vs cost)
/// P&L for an account from its transaction ledger (oldest->newest) and current valued holdings.
pub fn performance(txns: &[LedgerTxn], valued: &ValuedPortfolio) -> Performance {
    let mut shares: BTreeMap<String, f64> = BTreeMap::new();
    let mut avg_cost: BTreeMap<String, f64> = BTreeMap::new();
    let mut realized = 0.0;
    let mut dividends = 0.0;
    let mut fees = 0.0;
    for t in txns {
        let tk = t.ticker.to_uppercase();
        match t.kind.as_str() {
            "BUY" => {
                let s = shares.entry(tk.clone()).or_insert(0.0);
                let c = avg_cost.entry(tk.clone()).or_insert(0.0);
                let new_s = *s + t.shares;
                if new_s > 0.0 {
                    *c = (*s * *c + t.shares * t.price) / new_s;
                }
                *s = new_s;
            }
            "SELL" => {
                let c = avg_cost.get(&tk).copied().unwrap_or(t.price);
                realized += (t.price - c) * t.shares;
                *shares.entry(tk).or_insert(0.0) -= t.shares;
            }
            "DIVIDEND" => dividends += t.amount.abs(),
            "FEE" => fees += t.amount.abs(),
            _ => {}
        }
    }
    realized += dividends - fees;
    Performance {
        realized_pl: realized,
        unrealized_pl: valued.total_gain,
        dividends,
        fees,
        total_pl: realized + valued.total_gain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> Quotes {
        [("AAPL".into(), 100.0), ("MSFT".into(), 100.0)]
            .into_iter()
            .collect()
    }

    #[test]
    fn account_type_tax_status() {
        assert!(AccountType::RothIRA.is_tax_advantaged());
        assert!(!AccountType::Individual.is_tax_advantaged());
        assert_eq!(AccountType::JointJTWROS.as_str(), "Joint JTWROS");
    }

    #[test]
    fn values_positions_with_weights() {
        let pos = vec![
            Position {
                ticker: "AAPL".into(),
                shares: 30.0,
                cost_basis: 100.0,
            },
            Position {
                ticker: "MSFT".into(),
                shares: 10.0,
                cost_basis: 100.0,
            },
        ];
        let v = value_positions(&pos, &q(), 0.0);
        assert_eq!(v.total_value, 4000.0); // 3000 + 1000
        let aapl = &v.positions[0];
        assert_eq!(aapl.weight, Some(75.0));
        assert_eq!(aapl.gain, Some(0.0)); // price == cost
    }

    #[test]
    fn rebalances_to_target() {
        // 80/20 actual -> 60/40 target: sell AAPL, buy MSFT
        let pos = vec![
            Position {
                ticker: "AAPL".into(),
                shares: 80.0,
                cost_basis: 100.0,
            },
            Position {
                ticker: "MSFT".into(),
                shares: 20.0,
                cost_basis: 100.0,
            },
        ];
        let v = value_positions(&pos, &q(), 0.0);
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
        let trades = rebalance_trades(&v, &model, &q(), 1.0);
        let aapl = trades.iter().find(|t| t.ticker == "AAPL").unwrap();
        let msft = trades.iter().find(|t| t.ticker == "MSFT").unwrap();
        assert_eq!(aapl.action, TradeAction::Sell);
        assert_eq!(aapl.shares, 20.0); // 8000 -> 6000
        assert_eq!(msft.action, TradeAction::Buy);
        assert_eq!(msft.shares, 20.0); // 2000 -> 4000
    }

    #[test]
    fn performance_realized_and_unrealized() {
        let txns = vec![
            LedgerTxn {
                kind: "BUY".into(),
                ticker: "AAPL".into(),
                shares: 10.0,
                price: 100.0,
                amount: -1000.0,
            },
            LedgerTxn {
                kind: "BUY".into(),
                ticker: "AAPL".into(),
                shares: 10.0,
                price: 200.0,
                amount: -2000.0,
            },
            LedgerTxn {
                kind: "SELL".into(),
                ticker: "AAPL".into(),
                shares: 5.0,
                price: 300.0,
                amount: 1500.0,
            },
            LedgerTxn {
                kind: "DIVIDEND".into(),
                ticker: "AAPL".into(),
                shares: 0.0,
                price: 0.0,
                amount: 50.0,
            },
        ];
        let pos = vec![Position {
            ticker: "AAPL".into(),
            shares: 15.0,
            cost_basis: 150.0,
        }];
        let valued = value_positions(&pos, &q(), 0.0);
        let p = performance(&txns, &valued);
        assert_eq!(p.realized_pl, 800.0);
        assert_eq!(p.dividends, 50.0);
        assert_eq!(p.unrealized_pl, -750.0);
        assert_eq!(p.total_pl, 50.0);
    }
}
