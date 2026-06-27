//! Paper-trading engine (Phase 5). An order executes into a paper FILL at the live quote, posts
//! a TRANSACTION, and updates the HOLDING — proving a thesis without real money (PHILOSOPHY.md).
//! A hard, deterministic RISK GATE runs before any fill (IDEA.md danger rail): no AI in the loop
//! can bypass it.

use sqlx::Row;
use uuid::Uuid;

use crate::db::{Db, DbError};
use crate::domain::TradeAction;

#[derive(Debug, thiserror::Error)]
pub enum PaperError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error("risk gate rejected order: {0}")]
    RiskRejected(String),
    #[error("no quote for {0}")]
    NoQuote(String),
    #[error("account not found")]
    AccountNotFound,
}

/// Deterministic pre-trade risk limits. These are hard caps — the agent loop cannot widen them.
#[derive(Clone, Copy, Debug)]
pub struct RiskLimits {
    /// reject a single order whose notional exceeds this fraction of the account value.
    pub max_order_frac: f64,
    /// reject a SELL of more shares than the account holds (no shorting in paper book).
    pub allow_short: bool,
    /// reject a BUY that would overdraw cash beyond this fraction (1.0 = no overdraft).
    pub max_cash_use_frac: f64,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_order_frac: 0.25,
            allow_short: false,
            max_cash_use_frac: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    pub order_id: String,
    pub shares: f64,
    pub price: f64,
}

/// Run the risk gate for a proposed order against the account's current state.
/// Returns Ok(()) if admissible, Err(RiskRejected) otherwise. Pure decision over inputs.
pub fn risk_check(
    limits: &RiskLimits,
    action: TradeAction,
    shares: f64,
    price: f64,
    account_value: f64,
    cash: f64,
    held_shares: f64,
) -> Result<(), PaperError> {
    if shares <= 0.0 || price <= 0.0 {
        return Err(PaperError::RiskRejected("non-positive shares/price".into()));
    }
    let notional = shares * price;
    if account_value > 0.0 && notional > limits.max_order_frac * account_value {
        return Err(PaperError::RiskRejected(format!(
            "order notional {notional:.2} exceeds {:.0}% of account value",
            limits.max_order_frac * 100.0
        )));
    }
    match action {
        TradeAction::Sell if !limits.allow_short && shares > held_shares + 1e-9 => Err(
            PaperError::RiskRejected(format!("sell {shares} > held {held_shares} (no shorting)")),
        ),
        TradeAction::Buy if notional > limits.max_cash_use_frac * cash + 1e-9 => {
            Err(PaperError::RiskRejected(format!(
                "buy {notional:.2} exceeds available cash {cash:.2}"
            )))
        }
        _ => Ok(()),
    }
}

impl Db {
    /// Paper-execute an order: risk-check, create order+fill, post a transaction, update the
    /// holding + account cash. All in one transaction. Returns the fill.
    pub async fn paper_execute(
        &self,
        account_id: &str,
        action: TradeAction,
        ticker: &str,
        shares: f64,
        price: f64,
        limits: &RiskLimits,
    ) -> Result<Fill, PaperError> {
        let ticker = ticker.to_uppercase();
        // current account state
        let acct = sqlx::query("SELECT cash FROM account WHERE id = ?")
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(PaperError::AccountNotFound)?;
        let cash: f64 = acct.get("cash");
        let held: f64 = sqlx::query("SELECT COALESCE(SUM(shares),0.0) AS s FROM holding WHERE account_id = ? AND ticker = ?")
            .bind(account_id)
            .bind(&ticker)
            .fetch_one(&self.pool)
            .await?
            .get("s");
        // account value for the order-size cap: holdings priced at this order's price is a rough
        // bound; we use cash + held*price + this ticker only (sufficient for the size guard).
        let account_value = cash + held * price;

        risk_check(limits, action, shares, price, account_value, cash, held)?;

        let mut tx = self.pool.begin().await?;
        let order_id = Uuid::new_v4().to_string();
        let act = match action {
            TradeAction::Buy => "BUY",
            TradeAction::Sell => "SELL",
        };
        sqlx::query("INSERT INTO paper_order (id, account_id, action, ticker, shares, status) VALUES (?, ?, ?, ?, ?, 'PAPER_FILLED')")
            .bind(&order_id).bind(account_id).bind(act).bind(&ticker).bind(shares)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO paper_fill (id, order_id, shares, price) VALUES (?, ?, ?, ?)")
            .bind(Uuid::new_v4().to_string())
            .bind(&order_id)
            .bind(shares)
            .bind(price)
            .execute(&mut *tx)
            .await?;

        let notional = shares * price;
        let cash_delta = match action {
            TradeAction::Buy => -notional,
            TradeAction::Sell => notional,
        };
        sqlx::query("INSERT INTO transaction_ledger (id, account_id, kind, ticker, shares, price, amount) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(Uuid::new_v4().to_string()).bind(account_id).bind(act).bind(&ticker).bind(shares).bind(price).bind(cash_delta)
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE account SET cash = cash + ? WHERE id = ?")
            .bind(cash_delta)
            .bind(account_id)
            .execute(&mut *tx)
            .await?;

        // update or insert the holding (net shares; cost basis = weighted avg on buys)
        let signed = match action {
            TradeAction::Buy => shares,
            TradeAction::Sell => -shares,
        };
        let existing = sqlx::query(
            "SELECT id, shares, cost_basis FROM holding WHERE account_id = ? AND ticker = ?",
        )
        .bind(account_id)
        .bind(&ticker)
        .fetch_optional(&mut *tx)
        .await?;
        match existing {
            Some(row) => {
                let hid: String = row.get("id");
                let cur: f64 = row.get("shares");
                let cur_cost: f64 = row.get("cost_basis");
                let new_shares = cur + signed;
                let new_cost = if matches!(action, TradeAction::Buy) && new_shares > 0.0 {
                    (cur * cur_cost + shares * price) / new_shares
                } else {
                    cur_cost
                };
                if new_shares.abs() < 1e-9 {
                    sqlx::query("DELETE FROM holding WHERE id = ?")
                        .bind(&hid)
                        .execute(&mut *tx)
                        .await?;
                } else {
                    sqlx::query("UPDATE holding SET shares = ?, cost_basis = ? WHERE id = ?")
                        .bind(new_shares)
                        .bind(new_cost)
                        .bind(&hid)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            None => {
                sqlx::query("INSERT INTO holding (id, account_id, ticker, shares, cost_basis) VALUES (?, ?, ?, ?, ?)")
                    .bind(Uuid::new_v4().to_string()).bind(account_id).bind(&ticker).bind(signed).bind(price)
                    .execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(Fill {
            order_id,
            shares,
            price,
        })
    }

    /// Count of ledger transactions for an account (proof-of-activity).
    pub async fn transaction_count(&self, account_id: &str) -> Result<i64, sqlx::Error> {
        Ok(
            sqlx::query("SELECT COUNT(*) AS c FROM transaction_ledger WHERE account_id = ?")
                .bind(account_id)
                .fetch_one(&self.pool)
                .await?
                .get("c"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap;

    #[test]
    fn risk_gate_blocks_oversized_and_short_and_overdraft() {
        let l = RiskLimits::default();
        // oversized: 100 sh @ 100 = 10000 > 25% of 1000
        assert!(risk_check(&l, TradeAction::Buy, 100.0, 100.0, 1000.0, 1000.0, 0.0).is_err());
        // short sell: sell 10 when holding 0
        assert!(risk_check(&l, TradeAction::Sell, 10.0, 10.0, 100000.0, 0.0, 0.0).is_err());
        // overdraft buy: 50 sh @ 10 = 500 > 100 cash
        assert!(risk_check(&l, TradeAction::Buy, 50.0, 10.0, 100000.0, 100.0, 0.0).is_err());
        // admissible: 5 sh @ 10 = 50, within 25% of 100000, cash 1000
        assert!(risk_check(&l, TradeAction::Buy, 5.0, 10.0, 100000.0, 1000.0, 0.0).is_ok());
    }

    #[tokio::test]
    async fn paper_buy_then_sell_updates_ledger_and_holding() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        let hh = db.households().await.unwrap()[0].id.clone();
        let accts = db.accounts_for_household(&hh).await.unwrap();
        // Cole taxable has 2500 cash, no holdings
        let acct = accts.iter().find(|a| a.number == "INDV-COLE-001").unwrap();
        let limits = RiskLimits::default();

        // buy 5 VTI @ 200 = 1000 (within 25% guard? account_value = 2500 cash; 1000 <= 625? NO)
        // raise the guard for this test by buying smaller: 2 @ 200 = 400 <= 25% of 2500 = 625 ✓
        let fill = db
            .paper_execute(&acct.id, TradeAction::Buy, "VTI", 2.0, 200.0, &limits)
            .await
            .unwrap();
        assert_eq!(fill.shares, 2.0);
        assert_eq!(db.transaction_count(&acct.id).await.unwrap(), 1);

        // cash should be 2500 - 400 = 2100
        let q = [("VTI".into(), 200.0)].into_iter().collect();
        let valued = db.value_account(&acct.id, &q).await.unwrap();
        assert_eq!(valued.total_value, 400.0 + 2100.0); // holding + cash

        // sell the 2 shares back @ 250
        db.paper_execute(&acct.id, TradeAction::Sell, "VTI", 2.0, 250.0, &limits)
            .await
            .unwrap();
        let after = db.positions_for_account(&acct.id).await.unwrap();
        assert!(after.is_empty()); // position closed
        assert_eq!(db.transaction_count(&acct.id).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn risk_gate_rejects_in_engine() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        let hh = db.households().await.unwrap()[0].id.clone();
        let acct = db
            .accounts_for_household(&hh)
            .await
            .unwrap()
            .into_iter()
            .find(|a| a.number == "ROTH-COLE-001")
            .unwrap(); // 1000 cash
        let limits = RiskLimits::default();
        // buy 100 @ 100 = 10000 >> 25% of 1000 → rejected, no ledger entry
        let res = db
            .paper_execute(&acct.id, TradeAction::Buy, "AAPL", 100.0, 100.0, &limits)
            .await;
        assert!(matches!(res, Err(PaperError::RiskRejected(_))));
        assert_eq!(db.transaction_count(&acct.id).await.unwrap(), 0);
    }
}
