//! Formal-verification rail (Phase 7-8, IDEA.md §6.2). The risk gate (`paper::risk_check`) is the
//! safety-critical decision the whole agentic layer trusts: "prove the decision/risk logic before
//! real money." The 5 properties below are PROVEN in Rocq/Coq in `proofs/RiskGate.v` (machine-
//! checked: `coqc proofs/RiskGate.v`); this module is their executable model-check over Rust.
//! A full Coq development lives in `proofs/RiskGate.v` (see the proof obligations below);
//! here we provide the *executable* rail — exhaustive property checks over a dense input grid that
//! verify the gate's INVARIANTS hold, so a regression in `risk_check` fails CI immediately.
//!
//! ## Proof obligations (the Coq spec this rail approximates)
//! For all admissible (action, shares>0, price>0, account_value, cash, held):
//!   P1. No-short:        SELL with shares > held  ⇒ rejected (unless allow_short).
//!   P2. No-overdraft:    BUY with shares·price > max_cash_use_frac·cash  ⇒ rejected.
//!   P3. Size-cap:        shares·price > max_order_frac·account_value (>0)  ⇒ rejected.
//!   P4. Determinism:     risk_check is a pure function (same inputs ⇒ same verdict).
//!   P5. Monotone-size:   if an order is rejected for size, any larger order is too.
//! These are theorems to discharge in Coq; the tests below are their finite model-check.

#[cfg(test)]
mod proof_rail {
    use crate::domain::TradeAction;
    use crate::paper::{RiskLimits, risk_check};

    fn admitted(
        limits: &RiskLimits,
        a: TradeAction,
        shares: f64,
        price: f64,
        acct: f64,
        cash: f64,
        held: f64,
    ) -> bool {
        risk_check(limits, a, shares, price, acct, cash, held).is_ok()
    }

    /// Dense grid over the gate's inputs.
    fn grid() -> Vec<(f64, f64, f64, f64, f64)> {
        let vals = [0.0, 1.0, 5.0, 25.0, 100.0];
        let prices = [1.0, 10.0, 100.0];
        let mut out = vec![];
        for &shares in &vals {
            for &price in &prices {
                for &acct in &[0.0, 100.0, 1000.0, 100000.0] {
                    for &cash in &[0.0, 100.0, 10000.0] {
                        for &held in &vals {
                            out.push((shares, price, acct, cash, held));
                        }
                    }
                }
            }
        }
        out
    }

    #[test]
    fn p1_no_short_without_permission() {
        let l = RiskLimits {
            max_order_frac: 1.0,
            allow_short: false,
            max_cash_use_frac: 1.0,
        };
        for (shares, price, acct, cash, held) in grid() {
            if shares > 0.0 && price > 0.0 && shares > held {
                assert!(
                    !admitted(&l, TradeAction::Sell, shares, price, acct, cash, held),
                    "short sell admitted: {shares} > held {held}"
                );
            }
        }
    }

    #[test]
    fn p2_no_overdraft() {
        let l = RiskLimits {
            max_order_frac: 1.0,
            allow_short: true,
            max_cash_use_frac: 1.0,
        };
        for (shares, price, acct, cash, held) in grid() {
            let notional = shares * price;
            if shares > 0.0 && price > 0.0 && notional > cash + 1e-9 {
                assert!(
                    !admitted(&l, TradeAction::Buy, shares, price, acct, cash, held),
                    "overdraft admitted: {notional} > cash {cash}"
                );
            }
        }
    }

    #[test]
    fn p3_size_cap_enforced() {
        let l = RiskLimits {
            max_order_frac: 0.25,
            allow_short: true,
            max_cash_use_frac: 1e9,
        };
        for (shares, price, acct, _cash, held) in grid() {
            let notional = shares * price;
            let _ = held;
            if shares > 0.0 && price > 0.0 && acct > 0.0 && notional > 0.25 * acct {
                // a SELL of held shares (no short, no cash limit) isolates the size cap
                assert!(
                    !admitted(&l, TradeAction::Sell, shares, price, acct, 1e9, shares),
                    "oversized order admitted: {notional} > 25% of {acct}"
                );
            }
        }
    }

    #[test]
    fn p4_determinism() {
        let l = RiskLimits::default();
        for (shares, price, acct, cash, held) in grid() {
            let a = admitted(&l, TradeAction::Buy, shares, price, acct, cash, held);
            let b = admitted(&l, TradeAction::Buy, shares, price, acct, cash, held);
            assert_eq!(a, b);
        }
    }

    #[test]
    fn p5_monotone_in_size() {
        // if N shares is rejected for size, 2N is too (size cap is monotone)
        let l = RiskLimits {
            max_order_frac: 0.25,
            allow_short: true,
            max_cash_use_frac: 1e9,
        };
        for price in [1.0, 10.0, 100.0] {
            for acct in [100.0, 1000.0, 100000.0] {
                for n in [1.0, 5.0, 25.0, 100.0] {
                    let small = admitted(&l, TradeAction::Sell, n, price, acct, 1e9, 1e9);
                    let big = admitted(&l, TradeAction::Sell, n * 2.0, price, acct, 1e9, 1e9);
                    if !small {
                        assert!(
                            !big,
                            "monotonicity violated at n={n} price={price} acct={acct}"
                        );
                    }
                }
            }
        }
    }
}
