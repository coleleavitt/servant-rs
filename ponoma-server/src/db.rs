//! sqlx (SQLite) persistence for the ponoma book of record. Runtime queries (no compile-time
//! DB needed). Connect, migrate, seed, and read the household/account/holding spine.

use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

use crate::agent::{AdvisorReview, HarvestPlan, ReviewInputs, advisor_review, propose_harvest};
use crate::domain::{
    AccountType,
    LedgerTxn,
    ModelHolding,
    Performance,
    Position,
    Quotes,
    ValuedPortfolio,
    performance,
    value_positions,
};
use crate::paper::RiskLimits;
use crate::proposal::{Proposal, build_proposal};

/// The schema, embedded at compile time. Idempotent (`CREATE TABLE IF NOT EXISTS`), so applying
/// it on every boot is safe — we run it directly rather than via the `migrate!` macro (which
/// needs the extra `macros` feature + a build-time DB).
const SCHEMA_SQL: &str = include_str!("../migrations/0001_schema.sql");

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}

#[derive(Clone)]
pub struct Db {
    pub pool: SqlitePool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Household {
    pub id: String,
    pub name: String,
    pub advisor_rep: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub id: String,
    pub household_id: String,
    pub number: String,
    pub account_type: String,
    pub cash: f64,
    pub model_id: Option<String>,
}

impl Db {
    /// Connect to a SQLite URL (e.g. "sqlite::memory:" or "sqlite://ponoma.db?mode=rwc").
    pub async fn connect(url: &str) -> Result<Self, DbError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    /// Apply the embedded schema (idempotent). Strips `--` line comments, then runs each
    /// `;`-terminated statement in order. (A naive split-on-`;` would carry inline comment text
    /// like `-- signed cash impact` into the next statement and break parsing.)
    pub async fn migrate(&self) -> Result<(), DbError> {
        let cleaned: String = SCHEMA_SQL
            .lines()
            .map(|l| match l.find("--") {
                Some(i) => &l[..i],
                None => l,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for stmt in cleaned.split(';') {
            let s = stmt.trim();
            if s.is_empty() {
                continue;
            }
            // SAFETY (SqlSafeStr): SCHEMA_SQL is a trusted compile-time constant with no user
            // input — splitting it into statements cannot introduce injection.
            sqlx::query(sqlx::AssertSqlSafe(s.to_string()))
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn households(&self) -> Result<Vec<Household>, DbError> {
        let rows = sqlx::query("SELECT id, name, advisor_rep FROM household ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| Household {
                id: r.get("id"),
                name: r.get("name"),
                advisor_rep: r.get("advisor_rep"),
            })
            .collect())
    }

    pub async fn accounts_for_household(
        &self,
        household_id: &str,
    ) -> Result<Vec<Account>, DbError> {
        let rows = sqlx::query(
            "SELECT id, household_id, number, account_type, cash, model_id \
             FROM account WHERE household_id = ? ORDER BY number",
        )
        .bind(household_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| Account {
                id: r.get("id"),
                household_id: r.get("household_id"),
                number: r.get("number"),
                account_type: r.get("account_type"),
                cash: r.get("cash"),
                model_id: r.try_get("model_id").ok(),
            })
            .collect())
    }

    pub async fn positions_for_account(&self, account_id: &str) -> Result<Vec<Position>, DbError> {
        let rows =
            sqlx::query("SELECT ticker, shares, cost_basis FROM holding WHERE account_id = ?")
                .bind(account_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|r| Position {
                ticker: r.get("ticker"),
                shares: r.get("shares"),
                cost_basis: r.get("cost_basis"),
            })
            .collect())
    }

    /// Value an account from a quote map (its holdings + cash).
    pub async fn value_account(
        &self,
        account_id: &str,
        quotes: &Quotes,
    ) -> Result<ValuedPortfolio, DbError> {
        let positions = self.positions_for_account(account_id).await?;
        let cash: f64 = sqlx::query("SELECT cash FROM account WHERE id = ?")
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| r.get::<f64, _>("cash"))
            .unwrap_or(0.0);
        Ok(value_positions(&positions, quotes, cash))
    }

    /// Household AUM = sum of every account's market value (needs quotes for all held tickers).
    pub async fn household_aum(&self, household_id: &str, quotes: &Quotes) -> Result<f64, DbError> {
        let accounts = self.accounts_for_household(household_id).await?;
        let mut aum = 0.0;
        for a in accounts {
            aum += self.value_account(&a.id, quotes).await?.total_value;
        }
        Ok(aum)
    }

    /// Insert an audit event.
    pub async fn audit(
        &self,
        household_id: Option<&str>,
        action: &str,
        entity: &str,
        detail: &str,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO audit_event (id, household_id, action, entity, detail) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(household_id)
        .bind(action)
        .bind(entity)
        .bind(detail)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Create a household; returns its new id.
    pub async fn create_household(&self, name: &str, advisor_rep: &str) -> Result<String, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO household (id, name, advisor_rep) VALUES (?, ?, ?)")
            .bind(&id)
            .bind(name)
            .bind(advisor_rep)
            .execute(&self.pool)
            .await?;
        self.audit(Some(&id), "created", "household", name).await?;
        Ok(id)
    }

    /// Create an account under a household; returns its new id.
    pub async fn create_account(
        &self,
        household_id: &str,
        number: &str,
        account_type: &str,
        cash: f64,
    ) -> Result<String, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO account (id, household_id, number, account_type, cash) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(household_id)
        .bind(number)
        .bind(account_type)
        .bind(cash)
        .execute(&self.pool)
        .await?;
        self.audit(
            Some(household_id),
            "created",
            "account",
            &format!("{number} ({account_type})"),
        )
        .await?;
        Ok(id)
    }

    /// The transaction ledger for an account (newest first).
    pub async fn transactions_for_account(
        &self,
        account_id: &str,
    ) -> Result<Vec<Transaction>, DbError> {
        let rows = sqlx::query(
            "SELECT id, kind, ticker, shares, price, amount, at FROM transaction_ledger \
             WHERE account_id = ? ORDER BY at DESC, id DESC",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| Transaction {
                id: r.get("id"),
                kind: r.get("kind"),
                ticker: r.get("ticker"),
                shares: r.get("shares"),
                price: r.get("price"),
                amount: r.get("amount"),
                at: r.get("at"),
            })
            .collect())
    }

    /// Realized + unrealized P&L for an account (ledger replay + current holdings vs quotes).
    pub async fn account_performance(
        &self,
        account_id: &str,
        quotes: &Quotes,
    ) -> Result<Performance, DbError> {
        let valued = self.value_account(account_id, quotes).await?;
        let mut txns = self.transactions_for_account(account_id).await?;
        txns.reverse(); // transactions_for_account is newest-first; performance wants oldest-first
        let ledger: Vec<LedgerTxn> = txns
            .into_iter()
            .map(|t| LedgerTxn {
                kind: t.kind,
                ticker: t.ticker,
                shares: t.shares,
                price: t.price,
                amount: t.amount,
            })
            .collect();
        Ok(performance(&ledger, &valued))
    }

    /// Run the TLH agent on an account: load its type + valued holdings, propose harvest sells
    /// (only taxable accounts), each checked against the risk gate.
    pub async fn harvest_plan(
        &self,
        account_id: &str,
        quotes: &Quotes,
        min_loss_pct: f64,
    ) -> Result<HarvestPlan, DbError> {
        let valued = self.value_account(account_id, quotes).await?;
        let type_str: String = sqlx::query("SELECT account_type FROM account WHERE id = ?")
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| r.get::<String, _>("account_type"))
            .unwrap_or_else(|| "Individual".to_string());
        let at = AccountType::from_str_name(&type_str);
        Ok(propose_harvest(
            at,
            &valued,
            min_loss_pct,
            &RiskLimits::default(),
        ))
    }

    /// Run the AX-AI advisor review on an account: gather valuation + harvest plan + cash, then
    /// produce a prioritized recommendation list.
    pub async fn advisor_review(
        &self,
        account_id: &str,
        quotes: &Quotes,
    ) -> Result<AdvisorReview, DbError> {
        let valued = self.value_account(account_id, quotes).await?;
        let type_str: String = sqlx::query("SELECT account_type FROM account WHERE id = ?")
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| r.get::<String, _>("account_type"))
            .unwrap_or_else(|| "Individual".to_string());
        let at = AccountType::from_str_name(&type_str);
        let cash: f64 = sqlx::query("SELECT cash FROM account WHERE id = ?")
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| r.get::<f64, _>("cash"))
            .unwrap_or(0.0);
        let harvest = self.harvest_plan(account_id, quotes, -5.0).await?;
        let inp = ReviewInputs {
            account_type: at,
            valued: &valued,
            harvest: &harvest,
            concentration_pct: 40.0,
            cash_drag_pct: 10.0,
            cash,
        };
        Ok(advisor_review(&inp))
    }

    /// Generate an AI-copilot proposal: rebalance the account to `model`, summarized with drift
    /// reduction + tax impact. Taxable status comes from the account type.
    pub async fn proposal(
        &self,
        account_id: &str,
        model_name: &str,
        model: &[ModelHolding],
        quotes: &Quotes,
    ) -> Result<Proposal, DbError> {
        let valued = self.value_account(account_id, quotes).await?;
        let type_str: String = sqlx::query("SELECT account_type FROM account WHERE id = ?")
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| r.get::<String, _>("account_type"))
            .unwrap_or_else(|| "Individual".to_string());
        let taxable = !AccountType::from_str_name(&type_str).is_tax_advantaged();
        Ok(build_proposal(model_name, &valued, model, quotes, taxable))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
    pub id: String,
    pub kind: String,
    pub ticker: String,
    pub shares: f64,
    pub price: f64,
    pub amount: f64,
    pub at: String,
}
