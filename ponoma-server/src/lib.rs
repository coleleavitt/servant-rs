//! Ponoma backend — a multi-household book of record + paper-trading engine, on servant-rs +
//! sqlx. See `~/RustProjects/active/ponoma/{ARCHITECTURE,ROADMAP,PHILOSOPHY}.md`.
//!
//! Layers:
//! - [`domain`]  — pure types + analytics (mirrors the TS core; no I/O).
//! - [`db`]      — sqlx (SQLite) persistence of the household/account/holding spine.
//! - [`seed`]    — the real first household (Cole & Angelina).
//! - [`paper`]   — the paper-trading engine (order -> fill -> ledger) + risk gate.
//! - [`billing`] — fee-schedule calculation (Connect parity).
//! - [`mcp`]     — MCP tool surface over ponoma data (AI parity).
#![forbid(unsafe_code)]

pub mod agent;
pub mod billing;
pub mod db;
pub mod domain;
pub mod http;
pub mod mcp;
pub mod paper;
pub mod roles;
pub mod seed;
pub mod thesis;

pub use db::{Db, DbError};

/// Convenience: connect (in-memory or file), migrate, and seed in one call.
pub async fn bootstrap(url: &str) -> Result<Db, DbError> {
    let db = Db::connect(url).await?;
    db.migrate().await?;
    seed::seed_cole_and_angelina(&db).await?;
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Quotes;

    #[tokio::test]
    async fn bootstrap_seeds_household_and_values_it() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        let households = db.households().await.unwrap();
        assert_eq!(households.len(), 1);
        assert_eq!(households[0].name, "Cole & Angelina");

        let accounts = db.accounts_for_household(&households[0].id).await.unwrap();
        assert_eq!(accounts.len(), 5);
        assert!(accounts.iter().any(|a| a.account_type == "Roth IRA"));
        assert!(accounts.iter().any(|a| a.account_type == "Joint JTWROS"));

        // value the joint account: AAPL 20@200 + MSFT 10@400 + 5000 cash = 13000
        let joint = accounts.iter().find(|a| a.number == "JTWROS-001").unwrap();
        let quotes: Quotes = [("AAPL".into(), 200.0), ("MSFT".into(), 400.0)]
            .into_iter()
            .collect();
        let valued = db.value_account(&joint.id, &quotes).await.unwrap();
        assert_eq!(valued.total_value, 4000.0 + 4000.0 + 5000.0);

        // household AUM sums every account's value (other accounts are cash-only here)
        let aum = db.household_aum(&households[0].id, &quotes).await.unwrap();
        // 13000 (joint) + 2500 + 1000 + 1800 + 900 cash = 19200
        assert_eq!(aum, 19200.0);
    }

    #[tokio::test]
    async fn seed_is_idempotent() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        seed::seed_cole_and_angelina(&db).await.unwrap(); // second call no-ops
        assert_eq!(db.households().await.unwrap().len(), 1);
    }
}
