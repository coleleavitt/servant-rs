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

use crate::agent::{AdvisorReview, HarvestPlan};
use crate::billing::{FeeResult, FeeSchedule, compute_fee};
use crate::db::Db;
use crate::domain::{
    ModelHolding,
    Performance,
    Quotes,
    Trade,
    TradeAction,
    ValuedPortfolio,
    rebalance_trades,
};
use crate::mcp;
use crate::paper::{PaperError, RiskLimits};
use crate::proposal::Proposal;
use crate::thesis::{Signal as ThesisSignal, Thesis, Zone, synthesize};

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
#[derive(Deserialize)]
pub struct ProposalReq {
    pub account_id: String,
    pub model_name: String,
    pub model: Vec<ModelHolding>,
    pub quotes: std::collections::BTreeMap<String, f64>,
}
#[derive(Deserialize)]
pub struct NewHouseholdReq {
    pub name: String,
    #[serde(default)]
    pub advisor_rep: String,
}
#[derive(Deserialize)]
pub struct NewAccountReq {
    pub number: String,
    pub account_type: String,
    #[serde(default)]
    pub cash: f64,
}
#[derive(Deserialize)]
pub struct ThesisReq {
    pub ticker: String,
    pub zone: Zone,
    #[serde(default)]
    pub signals: Vec<ThesisSignal>,
}
#[derive(Serialize)]
pub struct CreatedDto {
    pub id: String,
}
#[derive(Serialize)]
pub struct TransactionDto {
    pub id: String,
    pub kind: String,
    pub ticker: String,
    pub shares: f64,
    pub price: f64,
    pub amount: f64,
    pub at: String,
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
            // POST /api/thesis  (ecosystem-understanding agent)
            path(
                "api",
                path(
                    "thesis",
                    req_body::<(Json,), ThesisReq, _>(post::<(Json,), Thesis>())
                )
            ),
            // POST /api/proposal  (AI copilot)
            path(
                "api",
                path(
                    "proposal",
                    req_body::<(Json,), ProposalReq, _>(post::<(Json,), Proposal>())
                )
            ),
            // POST /api/households  (create)
            path(
                "api",
                path(
                    "households",
                    req_body::<(Json,), NewHouseholdReq, _>(post::<(Json,), CreatedDto>())
                )
            ),
            // POST /api/households/{id}/accounts  (create)
            path(
                "api",
                path(
                    "households",
                    capture::<String, _>(
                        "id",
                        path(
                            "accounts",
                            req_body::<(Json,), NewAccountReq, _>(post::<(Json,), CreatedDto>())
                        )
                    )
                )
            ),
            // GET /api/accounts/{id}/transactions
            path(
                "api",
                path(
                    "accounts",
                    capture::<String, _>(
                        "id",
                        path("transactions", get::<(Json,), Vec<TransactionDto>>())
                    )
                )
            ),
            // GET /api/accounts/{id}/performance?q=
            path(
                "api",
                path(
                    "accounts",
                    capture::<String, _>(
                        "id",
                        path(
                            "performance",
                            query_param::<String, _>("q", get::<(Json,), Performance>())
                        )
                    )
                )
            ),
            // GET /api/accounts/{id}/harvest?q=
            path(
                "api",
                path(
                    "accounts",
                    capture::<String, _>(
                        "id",
                        path(
                            "harvest",
                            query_param::<String, _>("q", get::<(Json,), HarvestPlan>())
                        )
                    )
                )
            ),
            // GET /api/accounts/{id}/review?q=  (AX-AI advisor agent)
            path(
                "api",
                path(
                    "accounts",
                    capture::<String, _>(
                        "id",
                        path(
                            "review",
                            query_param::<String, _>("q", get::<(Json,), AdvisorReview>())
                        )
                    )
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

    let h_create_household = {
        let db = db.clone();
        move |req: NewHouseholdReq| {
            let db = db.clone();
            async move {
                let id = db
                    .create_household(&req.name, &req.advisor_rep)
                    .await
                    .map_err(db_err)?;
                Ok::<_, ServerError>(CreatedDto { id })
            }
        }
    };
    let h_create_account = {
        let db = db.clone();
        move |hid: String, req: NewAccountReq| {
            let db = db.clone();
            async move {
                let id = db
                    .create_account(&hid, &req.number, &req.account_type, req.cash)
                    .await
                    .map_err(db_err)?;
                Ok::<_, ServerError>(CreatedDto { id })
            }
        }
    };
    let h_transactions = {
        let db = db.clone();
        move |aid: String| {
            let db = db.clone();
            async move {
                let txns = db.transactions_for_account(&aid).await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    txns.into_iter()
                        .map(|t| TransactionDto {
                            id: t.id,
                            kind: t.kind,
                            ticker: t.ticker,
                            shares: t.shares,
                            price: t.price,
                            amount: t.amount,
                            at: t.at,
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }
    };

    let h_performance = {
        let db = db.clone();
        move |aid: String, q: Option<String>| {
            let db = db.clone();
            async move {
                db.account_performance(&aid, &parse_quotes(q))
                    .await
                    .map_err(db_err)
            }
        }
    };

    let h_harvest = {
        let db = db.clone();
        move |aid: String, q: Option<String>| {
            let db = db.clone();
            async move {
                db.harvest_plan(&aid, &parse_quotes(q), -5.0)
                    .await
                    .map_err(db_err)
            }
        }
    };

    let h_thesis = move |req: ThesisReq| async move {
        Ok::<_, ServerError>(synthesize(&req.ticker, req.zone, &req.signals))
    };

    let h_review = {
        let db = db.clone();
        move |aid: String, q: Option<String>| {
            let db = db.clone();
            async move {
                db.advisor_review(&aid, &parse_quotes(q))
                    .await
                    .map_err(db_err)
            }
        }
    };

    let h_proposal = {
        let db = db.clone();
        move |req: ProposalReq| {
            let db = db.clone();
            async move {
                let quotes: Quotes = req
                    .quotes
                    .into_iter()
                    .map(|(k, v)| (k.to_uppercase(), v))
                    .collect();
                db.proposal(&req.account_id, &req.model_name, &req.model, &quotes)
                    .await
                    .map_err(db_err)
            }
        }
    };

    // Handler tuple, right-nested to mirror the alt_all! tree (order = route declaration order).
    let handlers = (
        h_households,
        (
            h_accounts,
            (
                h_aum,
                (
                    h_holdings,
                    (
                        h_value,
                        (
                            h_tools,
                            (
                                h_paper,
                                (
                                    h_billing,
                                    (
                                        h_rebalance,
                                        (
                                            h_thesis,
                                            (
                                                h_proposal,
                                                (
                                                    h_create_household,
                                                    (
                                                        h_create_account,
                                                        (
                                                            h_transactions,
                                                            (h_performance, (h_harvest, h_review)),
                                                        ),
                                                    ),
                                                ),
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
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
    "POST /api/households            (create)",
    "POST /api/households/{id}/accounts (create)",
    "GET  /api/accounts/{id}/transactions",
    "GET  /api/accounts/{id}/performance?q=...",
    "GET  /api/accounts/{id}/harvest?q=...   (TLH agent)",
    "POST /api/thesis                       (ecosystem agent)",
    "GET  /api/accounts/{id}/review?q=...    (advisor agent)",
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

    #[tokio::test]
    async fn create_household_and_account_then_paper_trade_shows_in_ledger() {
        let c = client().await;
        // create a new household
        let created: serde_json::Value = c
            .request(http::Method::POST, "/api/households")
            .json(&serde_json::json!({"name": "Friends · Smith", "advisor_rep": "Cole"}))
            .send()
            .await
            .json();
        let hid = created["id"].as_str().unwrap().to_string();

        // it shows up in the list (now 2 households)
        let hs: Vec<HouseholdDtoOwned> = c.get("/api/households").await.json();
        assert_eq!(hs.len(), 2);

        // create an account under it
        let acc: serde_json::Value = c.request(http::Method::POST, &format!("/api/households/{hid}/accounts"))
            .json(&serde_json::json!({"number": "INDV-SMITH-001", "account_type": "Individual", "cash": 1000.0}))
            .send().await.json();
        let aid = acc["id"].as_str().unwrap().to_string();

        // ledger empty before any trade
        let txns: Vec<serde_json::Value> = c
            .get(&format!("/api/accounts/{aid}/transactions"))
            .await
            .json();
        assert_eq!(txns.len(), 0);

        // paper-trade (1 sh @ 200 = 200, within 25% of 1000 cash + risk gate), then ledger has it
        let r = c.request(http::Method::POST, "/api/paper-trade")
            .json(&serde_json::json!({"account_id": aid, "action": "BUY", "ticker": "VTI", "shares": 1.0, "price": 200.0}))
            .send().await;
        assert_eq!(r.status(), 200);
        let txns: Vec<serde_json::Value> = c
            .get(&format!("/api/accounts/{aid}/transactions"))
            .await
            .json();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0]["kind"], "BUY");
        assert_eq!(txns[0]["ticker"], "VTI");
        assert_eq!(txns[0]["amount"], -200.0); // cash out
    }

    #[tokio::test]
    async fn account_performance_endpoint() {
        let c = client().await;
        let hs: Vec<HouseholdDtoOwned> = c.get("/api/households").await.json();
        let accts: Vec<serde_json::Value> = c
            .get(&format!("/api/households/{}/accounts", hs[0].id))
            .await
            .json();
        // JTWROS holds AAPL 20@150 + MSFT 10@300; at AAPL=200/MSFT=400 unrealized = 1000+1000 = 2000
        let aid = accts.iter().find(|a| a["number"] == "JTWROS-001").unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let perf: serde_json::Value = c
            .get(&format!(
                "/api/accounts/{aid}/performance?q=AAPL:200,MSFT:400"
            ))
            .await
            .json();
        assert_eq!(perf["unrealized_pl"], 2000.0);
    }

    #[tokio::test]
    async fn thesis_endpoint() {
        let c = client().await;
        let r = c
            .request(http::Method::POST, "/api/thesis")
            .json(&serde_json::json!({
                "ticker": "WSHP",
                "zone": "Distress",
                "signals": [{"kind": "going-concern doubt", "bullish": false, "severity": 1.0}]
            }))
            .send()
            .await;
        assert_eq!(r.status(), 200);
        let t: serde_json::Value = r.json();
        assert_eq!(t["verdict"], "Avoid");
        assert_eq!(t["ticker"], "WSHP");
    }

    #[tokio::test]
    async fn advisor_review_endpoint() {
        let c = client().await;
        let hs: Vec<HouseholdDtoOwned> = c.get("/api/households").await.json();
        let accts: Vec<serde_json::Value> = c
            .get(&format!("/api/households/{}/accounts", hs[0].id))
            .await
            .json();
        // JTWROS: AAPL 20@150 + MSFT 10@300; crash AAPL to flag concentration + harvest
        let aid = accts.iter().find(|a| a["number"] == "JTWROS-001").unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let r: serde_json::Value = c
            .get(&format!("/api/accounts/{aid}/review?q=AAPL:50,MSFT:100"))
            .await
            .json();
        let kinds: Vec<&str> = r["recommendations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["kind"].as_str().unwrap())
            .collect();
        assert!(kinds.contains(&"harvest") || kinds.contains(&"concentration"));
    }

    #[tokio::test]
    async fn proposal_endpoint() {
        let c = client().await;
        let hs: Vec<HouseholdDtoOwned> = c.get("/api/households").await.json();
        let accts: Vec<serde_json::Value> = c
            .get(&format!("/api/households/{}/accounts", hs[0].id))
            .await
            .json();
        let aid = accts.iter().find(|a| a["number"] == "JTWROS-001").unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let r = c
            .request(http::Method::POST, "/api/proposal")
            .json(&serde_json::json!({
                "account_id": aid,
                "model_name": "Balanced",
                "model": [{"ticker": "AAPL", "target_weight": 50.0}, {"ticker": "MSFT", "target_weight": 50.0}],
                "quotes": {"AAPL": 200.0, "MSFT": 400.0}
            }))
            .send()
            .await;
        assert_eq!(r.status(), 200);
        let p: serde_json::Value = r.json();
        assert_eq!(p["model_name"], "Balanced");
        assert!(
            p["pre_active_share"].as_f64().unwrap() >= p["post_active_share"].as_f64().unwrap()
        );
    }

    // Owned mirror of HouseholdDto for deserializing in tests.
    #[derive(serde::Deserialize)]
    struct HouseholdDtoOwned {
        id: String,
        name: String,
    }
}
