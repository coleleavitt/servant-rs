//! Seed the real first household: "Cole & Angelina" with their accounts + a couple holdings.
//! This is a REAL household the tool manages (PHILOSOPHY.md) — replaceable/extendable, not a
//! synthetic client. Friends' households are added the same way.

use uuid::Uuid;

use crate::db::{Db, DbError};
use crate::domain::AccountType;

fn id() -> String {
    Uuid::new_v4().to_string()
}

/// Idempotent-ish seed: only runs if no household exists yet.
pub async fn seed_cole_and_angelina(db: &Db) -> Result<(), DbError> {
    if !db.households().await?.is_empty() {
        return Ok(());
    }
    let hh = id();
    sqlx::query(
        "INSERT INTO household (id, name, advisor_rep) VALUES (?, 'Cole & Angelina', 'Cole')",
    )
    .bind(&hh)
    .execute(&db.pool)
    .await?;

    // two clients in the household
    let cole = id();
    let angelina = id();
    for (cid, name, email) in [
        (&cole, "Cole", "cole@example.com"),
        (&angelina, "Angelina", "angelina@example.com"),
    ] {
        sqlx::query("INSERT INTO client (id, household_id, name, email) VALUES (?, ?, ?, ?)")
            .bind(cid)
            .bind(&hh)
            .bind(name)
            .bind(email)
            .execute(&db.pool)
            .await?;
    }

    let custodian = id();
    sqlx::query("INSERT INTO custodian (id, name) VALUES (?, 'Schwab')")
        .bind(&custodian)
        .execute(&db.pool)
        .await?;

    // accounts across registration/account types
    let accounts: &[(&str, AccountType, f64)] = &[
        ("INDV-COLE-001", AccountType::Individual, 2500.0),
        ("ROTH-COLE-001", AccountType::RothIRA, 1000.0),
        ("INDV-ANG-001", AccountType::Individual, 1800.0),
        ("ROTH-ANG-001", AccountType::RothIRA, 900.0),
        ("JTWROS-001", AccountType::JointJTWROS, 5000.0),
    ];
    for (num, at, cash) in accounts {
        let owner = if num.contains("ANG") {
            &angelina
        } else {
            &cole
        };
        seed_account(db, &hh, owner, &custodian, num, *at, *cash).await?;
    }

    db.audit(
        Some(&hh),
        "seeded",
        "household",
        "Cole & Angelina + 5 accounts",
    )
    .await?;
    Ok(())
}

/// Create one registration + account; seed the joint account with a couple holdings.
async fn seed_account(
    db: &Db,
    household_id: &str,
    owner_client: &str,
    custodian_id: &str,
    number: &str,
    at: AccountType,
    cash: f64,
) -> Result<(), DbError> {
    let aid = id();
    let reg = id();
    sqlx::query("INSERT INTO registration (id, client_id, reg_type) VALUES (?, ?, ?)")
        .bind(&reg)
        .bind(owner_client)
        .bind(at.as_str())
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "INSERT INTO account (id, household_id, registration_id, custodian_id, number, account_type, cash) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&aid)
    .bind(household_id)
    .bind(&reg)
    .bind(custodian_id)
    .bind(number)
    .bind(at.as_str())
    .bind(cash)
    .execute(&db.pool)
    .await?;
    if number == "JTWROS-001" {
        for (ticker, shares, cost) in [("AAPL", 20.0, 150.0), ("MSFT", 10.0, 300.0)] {
            sqlx::query(
                "INSERT INTO holding (id, account_id, ticker, shares, cost_basis) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id())
            .bind(&aid)
            .bind(ticker)
            .bind(shares)
            .bind(cost)
            .execute(&db.pool)
            .await?;
        }
    }
    Ok(())
}
