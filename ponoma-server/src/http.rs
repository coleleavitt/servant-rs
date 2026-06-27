//! The ponoma HTTP API — defined with servant-rs's typed combinators (the whole point of
//! building on servant-rs: one typed API description drives routing + extraction + content
//! negotiation). Handlers return `Result<T: Serialize as Json, ServerError>`. The binary
//! (`bin/ponoma_http.rs`) serves the resulting `RouterService` over servant's own hyper loop.
//!
//! Routes (all JSON):
//!   GET  /api/households
//!   GET  /api/households/{id}/accounts
//!   GET  /api/households/{id}/aum?q=AAPL:200,MSFT:400
//!   GET  /api/accounts/{id}/holdings
//!   GET  /api/accounts/{id}/value?q=...
//!   GET  /api/tools                       (MCP tool catalog)
//!   POST /api/paper-trade                 (through the risk gate)
//!   POST /api/billing
//!   POST /api/rebalance-preview

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use servant::alt_all;
use servant::prelude::*;
use servant_server::{RouterService, serve};

use crate::billing::{FeeResult, FeeSchedule, compute_fee};
use crate::db::Db;
use crate::domain::{ModelHolding, Quotes, Trade, TradeAction, ValuedPortfolio, rebalance_trades};
use crate::mcp;
use crate::paper::{PaperError, RiskLimits};

// ── wire DTOs (Serialize/Deserialize for Json) ───────────────────────────────
#[derive(Serialize)]
pub struct HouseholdDto {
    pub id: String,
    pub name: String,
    pub advisor_rep: String,
}
#[derive(Serialize)]
pub struct AccountDto {
    pub id: String,
    pub number: String,
    pub account_type: String,
    pub cash: f64,
    pub model_id: Option<String>,
}
#[derive(Serialize)]
pub struct HoldingDto {
    pub ticker: String,
    pub shares: f64,
    pub cost_basis: f64,
}
#[derive(Serialize)]
pub struct AumDto {
    pub aum: f64,
}
#[derive(Deserialize)]
pub struct PaperTradeReq {
    pub account_id: String,
    pub action: String,
    pub ticker: String,
    pub shares: f64,
    pub price: f64,
}
#[derive(Serialize)]
pub struct FillDto {
    pub order_id: String,
    pub shares: f64,
    pub price: f64,
}
#[derive(Deserialize)]
pub struct BillingReq {
    pub aum: f64,
    pub schedule: FeeSchedule,
}
#[derive(Deserialize)]
pub struct RebalanceReq {
    pub account_id: String,
    pub model: Vec<ModelHolding>,
    pub quotes: std::collections::BTreeMap<String, f64>,
}

/// Parse `?q=AAPL:200,MSFT:400` into a quote map.
fn parse_quotes(q: Option<String>) -> Quotes {
    q.map(|v| {
        v.split(',')
            .filter_map(|pair| {
                let (t, p) = pair.split_once(':')?;
                Some((t.to_uppercase(), p.parse().ok()?))
            })
            .collect()
    })
    .unwrap_or_default()
}

fn db_err(e: impl std::fmt::Display) -> ServerError {
    ServerError::err500().with_reason(e.to_string())
}

/// Build the typed API description (routing tree shape) — paired structurally with the handler
/// tuple in [`router`]. Each `path/capture/query/req_body` mirrors a route above.
// `Path` requires its child to be a `ServerKind`, and `Alt` is not one — so the `alt` must be at
// the top with each branch carrying the full `api/...` path (mirrors the todos_crud example).
macro_rules! ponoma_api {
    () => {
        alt_all![
            // GET /api/households
            path(
                "api",
                path("households", get::<(Json,), Vec<HouseholdDto>>())
            ),
            // GET /api/households/{id}/accounts
            path(
                "api",
                path(
                    "households",
                    capture::<String, _>("id", path("accounts", get::<(Json,), Vec<AccountDto>>()))
                )
            ),
            // GET /api/households/{id}/aum?q=
            path(
                "api",
                path(
                    "households",
                    capture::<String, _>(
                        "id",
                        path(
                            "aum",
                            query_param::<String, _>("q", get::<(Json,), AumDto>())
                        )
                    )
                )
            ),
            // GET /api/accounts/{id}/holdings
            path(
                "api",
                path(
                    "accounts",
                    capture::<String, _>("id", path("holdings", get::<(Json,), Vec<HoldingDto>>()))
                )
            ),
            // GET /api/accounts/{id}/value?q=
            path(
                "api",
                path(
                    "accounts",
                    capture::<String, _>(
                        "id",
                        path(
                            "value",
                            query_param::<String, _>("q", get::<(Json,), ValuedPortfolio>())
                        )
                    )
                )
            ),
            // GET /api/tools
            path("api", path("tools", get::<(Json,), serde_json::Value>())),
            // POST /api/paper-trade
            path(
                "api",
                path(
                    "paper-trade",
                    req_body::<(Json,), PaperTradeReq, _>(post::<(Json,), FillDto>())
                )
            ),
            // POST /api/billing
            path(
                "api",
                path(
                    "billing",
                    req_body::<(Json,), BillingReq, _>(post::<(Json,), FeeResult>())
                )
            ),
            // POST /api/rebalance-preview
            path(
                "api",
                path(
                    "rebalance-preview",
                    req_body::<(Json,), RebalanceReq, _>(post::<(Json,), Vec<Trade>>())
                )
            ),
        ]
    };
}

/// Build the served router for a database. The handler tuple is right-nested to match the
/// `alt_all!` tree shape (servant pairs them structurally).
pub fn router(db: Db) -> RouterService {
    let h_households = {
        let db = db.clone();
        move || {
            let db = db.clone();
            async move {
                let hs = db.households().await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    hs.into_iter()
                        .map(|h| HouseholdDto {
                            id: h.id,
                            name: h.name,
                            advisor_rep: h.advisor_rep,
                        })
                        .collect(),
                )
            }
        }
    };
    let h_accounts = {
        let db = db.clone();
        move |id: String| {
            let db = db.clone();
            async move {
                let a = db.accounts_for_household(&id).await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    a.into_iter()
                        .map(|a| AccountDto {
                            id: a.id,
                            number: a.number,
                            account_type: a.account_type,
                            cash: a.cash,
                            model_id: a.model_id,
                        })
                        .collect(),
                )
            }
        }
    };
    let h_aum = {
        let db = db.clone();
        move |id: String, q: Option<String>| {
            let db = db.clone();
            async move {
                Ok::<_, ServerError>(AumDto {
                    aum: db
                        .household_aum(&id, &parse_quotes(q))
                        .await
                        .map_err(db_err)?,
                })
            }
        }
    };
    let h_holdings = {
        let db = db.clone();
        move |id: String| {
            let db = db.clone();
            async move {
                let p = db.positions_for_account(&id).await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    p.into_iter()
                        .map(|p| HoldingDto {
                            ticker: p.ticker,
                            shares: p.shares,
                            cost_basis: p.cost_basis,
                        })
                        .collect(),
                )
            }
        }
    };
    let h_value = {
        let db = db.clone();
        move |id: String, q: Option<String>| {
            let db = db.clone();
            async move {
                db.value_account(&id, &parse_quotes(q))
                    .await
                    .map_err(db_err)
            }
        }
    };
    let h_tools = || async { Ok::<_, ServerError>(mcp::tools_list_json()) };
    let h_paper = {
        let db = db.clone();
        move |req: PaperTradeReq| {
            let db = db.clone();
            async move {
                let action = if req.action == "SELL" {
                    TradeAction::Sell
                } else {
                    TradeAction::Buy
                };
                match db
                    .paper_execute(
                        &req.account_id,
                        action,
                        &req.ticker,
                        req.shares,
                        req.price,
                        &RiskLimits::default(),
                    )
                    .await
                {
                    Ok(f) => Ok(FillDto {
                        order_id: f.order_id,
                        shares: f.shares,
                        price: f.price,
                    }),
                    Err(PaperError::RiskRejected(m)) => Err(ServerError::err422().with_reason(m)),
                    Err(e) => Err(db_err(e)),
                }
            }
        }
    };
    let h_billing =
        |req: BillingReq| async move { Ok::<_, ServerError>(compute_fee(&req.schedule, req.aum)) };
    let h_rebalance = {
        let db = db.clone();
        move |req: RebalanceReq| {
            let db = db.clone();
            async move {
                let quotes: Quotes = req
                    .quotes
                    .into_iter()
                    .map(|(k, v)| (k.to_uppercase(), v))
                    .collect();
                let valued = db
                    .value_account(&req.account_id, &quotes)
                    .await
                    .map_err(db_err)?;
                Ok::<_, ServerError>(rebalance_trades(&valued, &req.model, &quotes, 1.0))
            }
        }
    };

    // Handler tuple, right-nested to mirror the alt_all! tree.
    let handlers = (
        h_households,
        (
            h_accounts,
            (
                h_aum,
                (
                    h_holdings,
                    (h_value, (h_tools, (h_paper, (h_billing, h_rebalance)))),
                ),
            ),
        ),
    );
    RouterService::new(serve(ponoma_api!(), handlers))
}

/// The route list (for the binary's startup log) — the typed API shape, as text.
pub const ROUTES: &[&str] = &[
    "GET  /api/households",
    "GET  /api/households/{id}/accounts",
    "GET  /api/households/{id}/aum?q=AAPL:200,...",
    "GET  /api/accounts/{id}/holdings",
    "GET  /api/accounts/{id}/value?q=...",
    "GET  /api/tools",
    "POST /api/paper-trade",
    "POST /api/billing",
    "POST /api/rebalance-preview",
];

// Re-export Arc so the binary can share the service if needed.
pub type SharedService = Arc<RouterService>;

#[cfg(test)]
mod tests {
    use servant_server::TestClient;

    use super::*;
    use crate::bootstrap;

    async fn client() -> TestClient {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        TestClient::from_service(router(db))
    }

    #[tokio::test]
    async fn lists_households_and_accounts_typed() {
        let c = client().await;
        let hs: Vec<HouseholdDtoOwned> = c.get("/api/households").await.json();
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].name, "Cole & Angelina");

        let accts: Vec<serde_json::Value> = c
            .get(&format!("/api/households/{}/accounts", hs[0].id))
            .await
            .json();
        assert_eq!(accts.len(), 5);
        assert!(accts.iter().any(|a| a["account_type"] == "Roth IRA"));
    }

    #[tokio::test]
    async fn household_aum_via_query_param() {
        let c = client().await;
        let hs: Vec<HouseholdDtoOwned> = c.get("/api/households").await.json();
        let r = c
            .get(&format!(
                "/api/households/{}/aum?q=AAPL:200,MSFT:400",
                hs[0].id
            ))
            .await;
        assert_eq!(r.status(), 200);
        let aum: serde_json::Value = r.json();
        assert_eq!(aum["aum"], 19200.0);
    }

    #[tokio::test]
    async fn paper_trade_post_and_risk_reject() {
        let c = client().await;
        let hs: Vec<HouseholdDtoOwned> = c.get("/api/households").await.json();
        let accts: Vec<serde_json::Value> = c
            .get(&format!("/api/households/{}/accounts", hs[0].id))
            .await
            .json();
        let aid = accts
            .iter()
            .find(|a| a["number"] == "INDV-COLE-001")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let good = c.request(http::Method::POST, "/api/paper-trade")
            .json(&serde_json::json!({"account_id": aid, "action": "BUY", "ticker": "VTI", "shares": 2.0, "price": 200.0}))
            .send().await;
        assert_eq!(good.status(), 200);

        let bad = c.request(http::Method::POST, "/api/paper-trade")
            .json(&serde_json::json!({"account_id": aid, "action": "BUY", "ticker": "VTI", "shares": 1000.0, "price": 200.0}))
            .send().await;
        assert_eq!(bad.status(), 422); // risk-rejected through the typed API
    }

    // Owned mirror of HouseholdDto for deserializing in tests.
    #[derive(serde::Deserialize)]
    struct HouseholdDtoOwned {
        id: String,
        name: String,
    }
}
