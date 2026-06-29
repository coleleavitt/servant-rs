//! sqlx (SQLite) persistence for the ponoma book of record. Runtime queries (no compile-time
//! DB needed). Connect, migrate, seed, and read the household/account/holding spine.

use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

use crate::agent::{AdvisorReview, HarvestPlan, ReviewInputs, advisor_review, propose_harvest};
use crate::domain::{
    AccountType,
    LedgerTxn,
    Model,
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
    pub custodian: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Custodian {
    pub id: String,
    pub name: String,
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
            "SELECT a.id, a.household_id, a.number, a.account_type, a.cash, a.model_id, c.name AS custodian \
             FROM account a LEFT JOIN custodian c ON c.id = a.custodian_id \
             WHERE a.household_id = ? ORDER BY a.number",
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
                custodian: r.try_get("custodian").ok(),
            })
            .collect())
    }

    /// All custodians (reference list), alphabetical.
    pub async fn custodians(&self) -> Result<Vec<Custodian>, DbError> {
        let rows = sqlx::query("SELECT id, name FROM custodian ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| Custodian {
                id: r.get("id"),
                name: r.get("name"),
            })
            .collect())
    }

    /// Get-or-create a custodian by name; returns its id. Idempotent on name.
    pub async fn upsert_custodian(&self, name: &str) -> Result<String, DbError> {
        if let Some(row) = sqlx::query("SELECT id FROM custodian WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?
        {
            return Ok(row.get("id"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO custodian (id, name) VALUES (?, ?)")
            .bind(&id)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(id)
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
        custodian_id: Option<&str>,
    ) -> Result<String, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO account (id, household_id, number, account_type, cash, custodian_id) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(household_id)
        .bind(number)
        .bind(account_type)
        .bind(cash)
        .bind(custodian_id)
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

    /// Most-recent audit events (newest first), capped at `limit`.
    pub async fn recent_audit(&self, limit: i64) -> Result<Vec<AuditEvent>, DbError> {
        let rows = sqlx::query(
            "SELECT id, household_id, action, entity, detail, at FROM audit_event \
             ORDER BY at DESC, id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| AuditEvent {
                id: r.get("id"),
                household_id: r.try_get("household_id").ok(),
                action: r.get("action"),
                entity: r.get("entity"),
                detail: r.get("detail"),
                at: r.get("at"),
            })
            .collect())
    }

    /// Add a free-text note to an account; returns the new note id.
    pub async fn add_note(&self, account_id: &str, body: &str) -> Result<String, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO account_note (id, account_id, body) VALUES (?, ?, ?)")
            .bind(&id)
            .bind(account_id)
            .bind(body)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

     /// Notes for an account, newest first.
     pub async fn notes_for_account(&self, account_id: &str) -> Result<Vec<Note>, DbError> {
         let rows = sqlx::query(
             "SELECT id, body, at FROM account_note WHERE account_id = ? ORDER BY at DESC, id DESC",
         )
         .bind(account_id)
         .fetch_all(&self.pool)
         .await?;
         Ok(rows
             .into_iter()
             .map(|r| Note {
                 id: r.get("id"),
                 body: r.get("body"),
                 at: r.get("at"),
             })
             .collect())
     }

     /// Load a model's target holdings: (ticker, target_weight_percent).
     pub async fn model_targets(&self, model_id: &str) -> Result<Vec<(String, f64)>, DbError> {
         let rows = sqlx::query(
             "SELECT ticker, target_weight FROM model_holding WHERE model_id = ? ORDER BY ticker",
         )
         .bind(model_id)
         .fetch_all(&self.pool)
         .await?;
         Ok(rows
             .into_iter()
             .map(|r| (r.get::<String, _>("ticker").to_uppercase(), r.get("target_weight")))
             .collect())
     }

     /// Get an allocation's model_id via its strategy.
     pub async fn allocation_model_id(&self, allocation_id: &str) -> Result<Option<String>, DbError> {
         let row = sqlx::query(
             "SELECT ma.strategy_id FROM managed_allocation ma WHERE ma.id = ?",
         )
         .bind(allocation_id)
         .fetch_optional(&self.pool)
         .await?;
         if let Some(r) = row {
             let strategy_id: Option<String> = r.try_get("strategy_id").ok();
             if let Some(sid) = strategy_id {
                 let strategy_row = sqlx::query("SELECT model_id FROM strategy WHERE id = ?")
                     .bind(&sid)
                     .fetch_optional(&self.pool)
                     .await?;
              if let Some(sr) = strategy_row {
                      return Ok(sr.try_get("model_id").ok());
                  }
              }
          }
          Ok(None)
      }

    // ── Models (book-of-record target baskets; replaces the old localStorage models) ──────

    /// List all models with their holdings, newest first by name.
    pub async fn models(&self) -> Result<Vec<Model>, DbError> {
        let rows = sqlx::query("SELECT id, name FROM model ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let id: String = r.get("id");
            let holdings = self.model_targets(&id).await?;
            out.push(Model {
                id,
                name: r.get("name"),
                holdings: holdings
                    .into_iter()
                    .map(|(ticker, target_weight)| ModelHolding { ticker, target_weight })
                    .collect(),
            });
        }
        Ok(out)
    }

    /// Get one model with its holdings.
    pub async fn model(&self, id: &str) -> Result<Option<Model>, DbError> {
        let row = sqlx::query("SELECT id, name FROM model WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(r) = row else { return Ok(None) };
        let mid: String = r.get("id");
        let holdings = self.model_targets(&mid).await?;
        Ok(Some(Model {
            id: mid,
            name: r.get("name"),
            holdings: holdings
                .into_iter()
                .map(|(ticker, target_weight)| ModelHolding { ticker, target_weight })
                .collect(),
        }))
    }

    /// Create or replace a model + its holdings in one transaction. When `id` is provided and
    /// exists, the model is updated in place (holdings replaced); otherwise a new id is minted.
    /// Returns the model id.
    pub async fn upsert_model(
        &self,
        id: Option<&str>,
        name: &str,
        holdings: &[ModelHolding],
    ) -> Result<String, DbError> {
        let mid = id.map(str::to_owned).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO model (id, name) VALUES (?, ?) \
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
        )
        .bind(&mid)
        .bind(name)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM model_holding WHERE model_id = ?")
            .bind(&mid)
            .execute(&mut *tx)
            .await?;
        for h in holdings {
            let ticker = h.ticker.trim().to_uppercase();
            if ticker.is_empty() {
                continue;
            }
            sqlx::query(
                "INSERT INTO model_holding (model_id, ticker, target_weight) VALUES (?, ?, ?) \
                 ON CONFLICT(model_id, ticker) DO UPDATE SET target_weight = excluded.target_weight",
            )
            .bind(&mid)
            .bind(&ticker)
            .bind(h.target_weight)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.audit(None, if id.is_some() { "edited" } else { "created" }, "model", name)
            .await?;
        Ok(mid)
    }

    /// Delete a model (model_holding rows cascade).
    pub async fn delete_model(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM model WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.audit(None, "deleted", "model", id).await?;
        Ok(())
    }

    // ── app_kv: generic per-namespace JSON store (replaces remaining localStorage) ─────────

    /// All key/value pairs in a namespace.
    pub async fn kv_all(&self, namespace: &str) -> Result<Vec<KvEntry>, DbError> {
        let rows = sqlx::query("SELECT key, value FROM app_kv WHERE namespace = ? ORDER BY key")
            .bind(namespace)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| KvEntry { key: r.get("key"), value: r.get("value") })
            .collect())
    }

    /// Upsert one key in a namespace. `value` is an opaque JSON string.
    pub async fn kv_put(&self, namespace: &str, key: &str, value: &str) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO app_kv (namespace, key, value, updated_at) VALUES (?, ?, ?, datetime('now')) \
             ON CONFLICT(namespace, key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
        )
        .bind(namespace)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
  }

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: String,
    pub body: String,
    pub at: String,
}

/// One row of the generic app_kv store (value is an opaque JSON string).
#[derive(Clone, Debug, PartialEq)]
pub struct KvEntry {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AuditEvent {
    pub id: String,
    pub household_id: Option<String>,
    pub action: String,
    pub entity: String,
    pub detail: String,
    pub at: String,
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
