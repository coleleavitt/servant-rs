//! Billing (Phase 6, Connect parity). Fee-schedule calculation on AUM — tiered or flat basis,
//! with a minimum fee. This computes what an advisor *would* bill; it moves no money
//! (PHILOSOPHY.md: billing is analysis, not a custodial money-movement processor).

use serde::{Deserialize, Serialize};

/// One tier of a graduated fee schedule: applies `rate_pct` to AUM up to `up_to`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeeTier {
    /// upper bound (inclusive) of AUM this tier covers; use f64::INFINITY for the top tier.
    pub up_to: f64,
    /// annual rate in percent applied to the AUM that falls in this tier.
    pub rate_pct: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FeeBasis {
    /// graduated tiers: each slice of AUM is charged at its tier's rate.
    Tiered(Vec<FeeTier>),
    /// a single flat rate on all AUM.
    Flat { rate_pct: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeeSchedule {
    pub name: String,
    pub basis: FeeBasis,
    /// minimum annual fee (floor).
    pub minimum: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeeResult {
    /// annual fee in dollars.
    pub annual_fee: f64,
    /// effective blended rate as a percent of AUM.
    pub effective_rate_pct: f64,
    /// per-period (quarterly) fee.
    pub quarterly_fee: f64,
}

/// Compute the annual fee for an AUM under a schedule. Tiered fees are graduated (marginal):
/// each slice of AUM is charged at its own tier's rate, like income-tax brackets.
pub fn compute_fee(schedule: &FeeSchedule, aum: f64) -> FeeResult {
    let aum = aum.max(0.0);
    let raw = match &schedule.basis {
        FeeBasis::Flat { rate_pct } => aum * rate_pct / 100.0,
        FeeBasis::Tiered(tiers) => {
            let mut fee = 0.0;
            let mut prev = 0.0;
            for t in tiers {
                if aum <= prev {
                    break;
                }
                let slice = aum.min(t.up_to) - prev;
                if slice > 0.0 {
                    fee += slice * t.rate_pct / 100.0;
                }
                prev = t.up_to;
            }
            fee
        }
    };
    let annual_fee = raw.max(schedule.minimum);
    FeeResult {
        annual_fee,
        effective_rate_pct: if aum > 0.0 { annual_fee / aum * 100.0 } else { 0.0 },
        quarterly_fee: annual_fee / 4.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiered() -> FeeSchedule {
        FeeSchedule {
            name: "Standard".into(),
            basis: FeeBasis::Tiered(vec![
                FeeTier { up_to: 1_000_000.0, rate_pct: 1.0 },
                FeeTier { up_to: 5_000_000.0, rate_pct: 0.75 },
                FeeTier { up_to: f64::INFINITY, rate_pct: 0.5 },
            ]),
            minimum: 1_000.0,
        }
    }

    #[test]
    fn flat_fee() {
        let s = FeeSchedule { name: "Flat".into(), basis: FeeBasis::Flat { rate_pct: 1.0 }, minimum: 0.0 };
        let r = compute_fee(&s, 500_000.0);
        assert_eq!(r.annual_fee, 5_000.0);
        assert_eq!(r.effective_rate_pct, 1.0);
        assert_eq!(r.quarterly_fee, 1_250.0);
    }

    #[test]
    fn tiered_is_graduated() {
        // 2,000,000: first 1M @ 1% = 10,000; next 1M @ 0.75% = 7,500 → 17,500
        let r = compute_fee(&tiered(), 2_000_000.0);
        assert_eq!(r.annual_fee, 17_500.0);
        assert!((r.effective_rate_pct - 0.875).abs() < 1e-9);
    }

    #[test]
    fn minimum_fee_floor_applies() {
        // 50,000 @ 1% = 500, but minimum is 1,000 → 1,000
        let r = compute_fee(&tiered(), 50_000.0);
        assert_eq!(r.annual_fee, 1_000.0);
    }

    #[test]
    fn zero_aum_is_minimum() {
        assert_eq!(compute_fee(&tiered(), 0.0).annual_fee, 1_000.0);
    }
}
