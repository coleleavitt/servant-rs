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
    // Seed starter models independently of the household guard so existing DBs (which already
    // have the household but no models) get them too. Idempotent: only when no models exist.
    seed_starter_models(db).await?;

    if !db.households().await?.is_empty() {
        // Household already exists; just seed the managed allocation (idempotent).
        seed_managed_allocation(db).await?;
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

    // Reference custodians (the common RIA custodians). Cole & Angelina custody at Schwab.
    for name in ["Schwab", "Fidelity", "Pershing", "Altruist"] {
        db.upsert_custodian(name).await?;
    }
    let custodian = db.upsert_custodian("Schwab").await?;

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
    
    // Seed a managed allocation on one of the accounts.
    seed_managed_allocation(db).await?;
    Ok(())
}

/// Seed a couple of starter target baskets so the Models page isn't empty on first run.
/// Idempotent: no-op once any model exists.
async fn seed_starter_models(db: &Db) -> Result<(), DbError> {
    use crate::domain::ModelHolding;
    if !db.models().await?.is_empty() {
        return Ok(());
    }
    let mh = |t: &str, w: f64| ModelHolding { ticker: t.into(), target_weight: w };
    db.upsert_model(
        None,
        "Core Growth 60/40",
        &[mh("AAPL", 30.0), mh("MSFT", 30.0), mh("VTI", 25.0), mh("BND", 15.0)],
    )
    .await?;
    db.upsert_model(
        None,
        "Dividend Income",
        &[mh("SCHD", 40.0), mh("JNJ", 20.0), mh("PG", 20.0), mh("KO", 20.0)],
    )
    .await?;
    Ok(())
}

/// Seed a managed allocation on an existing account (so the Allocations page shows something).
/// Idempotent: only if no managed_allocation exists yet.
async fn seed_managed_allocation(db: &Db) -> Result<(), DbError> {
    // Only seed if no managed allocations exist yet.
    let households = db.households().await?;
    if households.is_empty() {
        return Ok(()); // No household to seed for
    }
    let hh = &households[0];
    
    // Check if there's already a managed allocation.
    let existing = db.allocations_for_household(&hh.id).await?;
    if !existing.is_empty() {
        return Ok(()); // Already have allocations
    }

    // Get the first account in the household.
    let accounts = db.accounts_for_household(&hh.id).await?;
    if accounts.is_empty() {
        return Ok(()); // No accounts to manage
    }
    let account = &accounts[0];

    // Get the first model (starter model).
    let models = db.models().await?;
    if models.is_empty() {
        return Ok(()); // No models available
    }
    let model = &models[0];

    // Create a strategy for this model.
    let strategy_id = db
        .create_strategy(
            &format!("Managed: {}", model.name),
            "standard",
            &format!("Manage {} toward {}", account.number, model.name),
            Some(&model.id),
            "{}",
        )
        .await?;

    // Put the account under management.
    db.manage_existing_account(
        &hh.id,
        &account.id,
        &format!("Managed: {}", account.number),
        "Agent-managed allocation",
        3,
        Some(&strategy_id),
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
