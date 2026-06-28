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
use crate::communities::{
    ModelComparison,
    StrategistModel,
    compare as community_compare,
    search as community_search,
    seed_models,
};
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
use crate::prospect::{ModelFit, ProspectProfile, rank_models};
use crate::roles::{Role, capabilities};
use crate::servicing::{ScheduledJob, TOPICS, default_jobs};
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
    pub custodian: Option<String>,
}
#[derive(Serialize)]
pub struct CustodianDto {
    pub id: String,
    pub name: String,
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
pub struct CompareReq {
    pub a: StrategistModel,
    pub b: StrategistModel,
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
    /// Optional custodian — by id (preferred) or by name (get-or-created).
    #[serde(default)]
    pub custodian_id: Option<String>,
    #[serde(default)]
    pub custodian: Option<String>,
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

// ── Phase 9: managed allocations, strategies, the agent loop ────────────────
#[derive(Serialize)]
pub struct AllocationDto {
    pub id: String,
    pub name: String,
    pub mandate: String,
    pub risk_level: i64,
    pub funded: f64,
    pub paper_account_id: String,
    pub strategy_id: Option<String>,
    pub active: bool,
}
#[derive(Deserialize)]
pub struct NewAllocationReq {
    pub name: String,
    #[serde(default)]
    pub mandate: String,
    #[serde(default = "default_risk")]
    pub risk_level: i64,
    pub funded: f64,
    #[serde(default)]
    pub source_account_id: Option<String>,
    #[serde(default)]
    pub strategy_id: Option<String>,
}
fn default_risk() -> i64 {
    3
}
#[derive(Serialize)]
pub struct StrategyDto {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub model_id: Option<String>,
    pub rules: String,
    pub version: i64,
}
#[derive(Deserialize)]
pub struct NewStrategyReq {
    pub name: String,
    #[serde(default = "default_strategy_kind")]
    pub kind: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default = "default_rules")]
    pub rules: String,
}
fn default_strategy_kind() -> String {
    "rotation".into()
}
fn default_rules() -> String {
    "{}".into()
}
#[derive(Serialize)]
pub struct DecisionDto {
    pub step: String,
    pub ticker: String,
    pub action: String,
    pub shares: f64,
    pub thesis: String,
    pub confidence: f64,
    pub admitted: bool,
    pub verdict: String,
    pub at: String,
}
#[derive(Deserialize)]
pub struct LoopTickReq {
    /// candidate tickers to evaluate this tick
    pub candidates: Vec<String>,
    /// distress zone per ticker ("safe"/"grey"/"distress"/"unknown")
    #[serde(default)]
    pub zones: std::collections::HashMap<String, String>,
    /// live quotes "AAPL:200,MSFT:400"
    #[serde(default)]
    pub quotes: String,
}
#[derive(Serialize)]
pub struct TickResultDto {
    pub ticker: String,
    pub action: String,
    pub admitted: bool,
    pub filled: bool,
    pub thesis: String,
    pub verdict: String,
}
#[derive(Deserialize)]
pub struct SetActiveReq {
    pub active: bool,
}
#[derive(Serialize)]
pub struct ImprovementDto {
    pub id: String,
    pub strategy_id: Option<String>,
    pub rationale: String,
    pub current_rules: String,
    pub proposed_rules: String,
    pub status: String,
}
#[derive(Deserialize)]
pub struct ResolveImprovementReq {
    pub approve: bool,
}
#[derive(Serialize)]
pub struct CapabilityDto {
    pub capability: String,
    pub allowed: bool,
}
#[derive(Serialize)]
pub struct NoteDto {
    pub id: String,
    pub body: String,
    pub at: String,
}
#[derive(Deserialize)]
pub struct NewNoteReq {
    pub body: String,
}
#[derive(Serialize)]
pub struct AuditDto {
    pub action: String,
    pub entity: String,
    pub detail: String,
    pub at: String,
    pub household_id: Option<String>,
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
            // GET /api/custodians
            path(
                "api",
                path("custodians", get::<(Json,), Vec<CustodianDto>>())
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
            // GET /api/servicing/topics
            path(
                "api",
                path("servicing", path("topics", get::<(Json,), Vec<String>>()))
            ),
            // GET /api/servicing/jobs
            path(
                "api",
                path(
                    "servicing",
                    path("jobs", get::<(Json,), Vec<ScheduledJob>>())
                )
            ),
            // GET /api/communities/models?q=
            path(
                "api",
                path(
                    "communities",
                    path(
                        "models",
                        query_param::<String, _>("q", get::<(Json,), Vec<StrategistModel>>())
                    )
                )
            ),
            // POST /api/communities/compare
            path(
                "api",
                path(
                    "communities",
                    path(
                        "compare",
                        req_body::<(Json,), CompareReq, _>(post::<(Json,), ModelComparison>())
                    )
                )
            ),
            // GET /api/audit
            path("api", path("audit", get::<(Json,), Vec<AuditDto>>())),
            // GET /api/accounts/{id}/notes
            path(
                "api",
                path(
                    "accounts",
                    capture::<String, _>("id", path("notes", get::<(Json,), Vec<NoteDto>>()))
                )
            ),
            // POST /api/accounts/{id}/notes
            path(
                "api",
                path(
                    "accounts",
                    capture::<String, _>(
                        "id",
                        path(
                            "notes",
                            req_body::<(Json,), NewNoteReq, _>(post::<(Json,), CreatedDto>())
                        )
                    )
                )
            ),
            // GET /api/roles/{role}/capabilities
            path(
                "api",
                path(
                    "roles",
                    capture::<String, _>(
                        "role",
                        path("capabilities", get::<(Json,), Vec<CapabilityDto>>())
                    )
                )
            ),
            // POST /api/prospect  (AI copilot prospecting)
            path(
                "api",
                path(
                    "prospect",
                    req_body::<(Json,), ProspectProfile, _>(post::<(Json,), Vec<ModelFit>>())
                )
            ),
            // GET /api/strategies
            path("api", path("strategies", get::<(Json,), Vec<StrategyDto>>())),
            // POST /api/strategies  (create)
            path(
                "api",
                path(
                    "strategies",
                    req_body::<(Json,), NewStrategyReq, _>(post::<(Json,), CreatedDto>())
                )
            ),
            // GET /api/households/{id}/allocations
            path(
                "api",
                path(
                    "households",
                    capture::<String, _>("id", path("allocations", get::<(Json,), Vec<AllocationDto>>()))
                )
            ),
            // POST /api/households/{id}/allocations  (create / fund)
            path(
                "api",
                path(
                    "households",
                    capture::<String, _>(
                        "id",
                        path(
                            "allocations",
                            req_body::<(Json,), NewAllocationReq, _>(post::<(Json,), CreatedDto>())
                        )
                    )
                )
            ),
            // GET /api/allocations/{id}/decisions
            path(
                "api",
                path(
                    "allocations",
                    capture::<String, _>("id", path("decisions", get::<(Json,), Vec<DecisionDto>>()))
                )
            ),
            // POST /api/allocations/{id}/tick  (run one agent loop iteration)
            path(
                "api",
                path(
                    "allocations",
                    capture::<String, _>(
                        "id",
                        path(
                            "tick",
                            req_body::<(Json,), LoopTickReq, _>(post::<(Json,), Vec<TickResultDto>>())
                        )
                    )
                )
            ),
            // POST /api/allocations/{id}/active  (kill switch)
            path(
                "api",
                path(
                    "allocations",
                    capture::<String, _>(
                        "id",
                        path(
                            "active",
                            req_body::<(Json,), SetActiveReq, _>(post::<(Json,), CreatedDto>())
                        )
                    )
                )
            ),
            // POST /api/allocations/{id}/self-improve  (propose a refinement from outcomes)
            path(
                "api",
                path(
                    "allocations",
                    capture::<String, _>("id", path("self-improve", post::<(Json,), CreatedDto>()))
                )
            ),
            // GET /api/allocations/{id}/improvements
            path(
                "api",
                path(
                    "allocations",
                    capture::<String, _>("id", path("improvements", get::<(Json,), Vec<ImprovementDto>>()))
                )
            ),
            // POST /api/improvements/{id}/resolve  (human approve/reject)
            path(
                "api",
                path(
                    "improvements",
                    capture::<String, _>(
                        "id",
                        path(
                            "resolve",
                            req_body::<(Json,), ResolveImprovementReq, _>(post::<(Json,), CreatedDto>())
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
    let h_custodians = {
        let db = db.clone();
        move || {
            let db = db.clone();
            async move {
                let cs = db.custodians().await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    cs.into_iter()
                        .map(|c| CustodianDto {
                            id: c.id,
                            name: c.name,
                        })
                        .collect::<Vec<_>>(),
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
                            custodian: a.custodian,
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
                // Resolve the custodian: explicit id wins; else get-or-create by name.
                let custodian_id = match (req.custodian_id, req.custodian) {
                    (Some(id), _) => Some(id),
                    (None, Some(name)) if !name.trim().is_empty() => {
                        Some(db.upsert_custodian(name.trim()).await.map_err(db_err)?)
                    }
                    _ => None,
                };
                let id = db
                    .create_account(
                        &hid,
                        &req.number,
                        &req.account_type,
                        req.cash,
                        custodian_id.as_deref(),
                    )
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

    let h_topics =
        || async { Ok::<_, ServerError>(TOPICS.iter().map(|t| t.to_string()).collect::<Vec<_>>()) };
    let h_jobs = || async { Ok::<_, ServerError>(default_jobs()) };
    let h_comm_models = move |q: Option<String>| async move {
        let models = seed_models();
        let filtered = community_search(&models, q.as_deref().unwrap_or(""))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        Ok::<_, ServerError>(filtered)
    };
    let h_comm_compare = move |req: CompareReq| async move {
        Ok::<_, ServerError>(community_compare(&req.a, &req.b))
    };

    let h_audit = {
        let db = db.clone();
        move || {
            let db = db.clone();
            async move {
                let events = db.recent_audit(100).await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    events
                        .into_iter()
                        .map(|e| AuditDto {
                            action: e.action,
                            entity: e.entity,
                            detail: e.detail,
                            at: e.at,
                            household_id: e.household_id,
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }
    };

    let h_notes_get = {
        let db = db.clone();
        move |aid: String| {
            let db = db.clone();
            async move {
                let notes = db.notes_for_account(&aid).await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    notes
                        .into_iter()
                        .map(|n| NoteDto {
                            id: n.id,
                            body: n.body,
                            at: n.at,
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }
    };
    let h_notes_add = {
        let db = db.clone();
        move |aid: String, req: NewNoteReq| {
            let db = db.clone();
            async move {
                let id = db.add_note(&aid, &req.body).await.map_err(db_err)?;
                Ok::<_, ServerError>(CreatedDto { id })
            }
        }
    };

    let h_caps = move |role: String| async move {
        let caps = capabilities(Role::from_str_name(&role));
        Ok::<_, ServerError>(
            caps.into_iter()
                .map(|(capability, allowed)| CapabilityDto {
                    capability: capability.to_string(),
                    allowed,
                })
                .collect::<Vec<_>>(),
        )
    };

    let h_prospect = move |profile: ProspectProfile| async move {
        Ok::<_, ServerError>(rank_models(&profile, &crate::communities::seed_models()))
    };

    // ── Phase 9 handlers: managed allocations, strategies, the agent loop ────
    let h_strategies = {
        let db = db.clone();
        move || {
            let db = db.clone();
            async move {
                let ss = db.strategies().await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    ss.into_iter()
                        .map(|s| StrategyDto {
                            id: s.id,
                            name: s.name,
                            kind: s.kind,
                            description: s.description,
                            model_id: s.model_id,
                            rules: s.rules,
                            version: s.version,
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }
    };
    let h_create_strategy = {
        let db = db.clone();
        move |req: NewStrategyReq| {
            let db = db.clone();
            async move {
                let id = db
                    .create_strategy(&req.name, &req.kind, &req.description, req.model_id.as_deref(), &req.rules)
                    .await
                    .map_err(db_err)?;
                Ok::<_, ServerError>(CreatedDto { id })
            }
        }
    };
    let h_allocations = {
        let db = db.clone();
        move |hid: String| {
            let db = db.clone();
            async move {
                let al = db.allocations_for_household(&hid).await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    al.into_iter()
                        .map(|a| AllocationDto {
                            id: a.id,
                            name: a.name,
                            mandate: a.mandate,
                            risk_level: a.risk_level,
                            funded: a.funded,
                            paper_account_id: a.paper_account_id,
                            strategy_id: a.strategy_id,
                            active: a.active,
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }
    };
    let h_create_allocation = {
        let db = db.clone();
        move |hid: String, req: NewAllocationReq| {
            let db = db.clone();
            async move {
                let id = db
                    .create_allocation(
                        &hid,
                        &req.name,
                        &req.mandate,
                        req.risk_level,
                        req.funded,
                        req.source_account_id.as_deref(),
                        req.strategy_id.as_deref(),
                    )
                    .await
                    .map_err(db_err)?;
                Ok::<_, ServerError>(CreatedDto { id })
            }
        }
    };
    let h_decisions = {
        let db = db.clone();
        move |aid: String| {
            let db = db.clone();
            async move {
                let ds = db.decisions_for_allocation(&aid, 100).await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    ds.into_iter()
                        .map(|d| DecisionDto {
                            step: d.step,
                            ticker: d.ticker,
                            action: d.action,
                            shares: d.shares,
                            thesis: d.thesis,
                            confidence: d.confidence,
                            admitted: d.admitted,
                            verdict: d.verdict,
                            at: d.at,
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }
    };
    let h_loop_tick = {
        let db = db.clone();
        move |aid: String, req: LoopTickReq| {
            let db = db.clone();
            async move {
                let zones: std::collections::HashMap<String, Zone> = req
                    .zones
                    .into_iter()
                    .map(|(t, z)| {
                        let zone = match z.to_ascii_lowercase().as_str() {
                            "safe" => Zone::Safe,
                            "grey" | "gray" => Zone::Grey,
                            "distress" => Zone::Distress,
                            _ => Zone::Unknown,
                        };
                        (t.to_uppercase(), zone)
                    })
                    .collect();
                let quotes = parse_quotes(Some(req.quotes));
                let results = db
                    .run_loop_tick(&aid, &req.candidates, &zones, &quotes)
                    .await
                    .map_err(|e| ServerError::err422().with_reason(e.to_string()))?;
                Ok::<_, ServerError>(
                    results
                        .into_iter()
                        .map(|r| TickResultDto {
                            ticker: r.ticker,
                            action: r.action,
                            admitted: r.admitted,
                            filled: r.filled,
                            thesis: r.thesis,
                            verdict: r.verdict,
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }
    };
    let h_set_active = {
        let db = db.clone();
        move |aid: String, req: SetActiveReq| {
            let db = db.clone();
            async move {
                db.set_allocation_active(&aid, req.active).await.map_err(db_err)?;
                Ok::<_, ServerError>(CreatedDto { id: aid })
            }
        }
    };
    // RSI (rails-respecting): propose a refinement from outcomes, list proposals, approve/reject.
    let h_propose_improvement = {
        let db = db.clone();
        move |aid: String| {
            let db = db.clone();
            async move {
                let id = db.propose_improvement(&aid).await.map_err(db_err)?;
                Ok::<_, ServerError>(CreatedDto { id: id.unwrap_or_default() })
            }
        }
    };
    let h_improvements = {
        let db = db.clone();
        move |aid: String| {
            let db = db.clone();
            async move {
                let ims = db.improvements_for_allocation(&aid).await.map_err(db_err)?;
                Ok::<_, ServerError>(
                    ims.into_iter()
                        .map(|i| ImprovementDto {
                            id: i.id,
                            strategy_id: i.strategy_id,
                            rationale: i.rationale,
                            current_rules: i.current_rules,
                            proposed_rules: i.proposed_rules,
                            status: i.status,
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }
    };
    let h_resolve_improvement = {
        let db = db.clone();
        move |iid: String, req: ResolveImprovementReq| {
            let db = db.clone();
            async move {
                db.resolve_improvement(&iid, req.approve).await.map_err(db_err)?;
                Ok::<_, ServerError>(CreatedDto { id: iid })
            }
        }
    };

    // Handler tuple, right-nested to mirror the alt_all! tree (order = route declaration order).
    let handlers = (
        h_households,
        (
            h_custodians,
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
                                                            (
                                                                h_performance,
                                                                (
                                                                    h_harvest,
                                                                    (
                                                                        h_review,
                                                                        (
                                                                            h_topics,
                                                                            (
                                                                                h_jobs,
                                                                                (
                                                                                    h_comm_models,
                                                                                    (h_comm_compare, (h_audit, (h_notes_get, (h_notes_add, (h_caps, (h_prospect, (h_strategies, (h_create_strategy, (h_allocations, (h_create_allocation, (h_decisions, (h_loop_tick, (h_set_active, (h_propose_improvement, (h_improvements, h_resolve_improvement)))))))))))))))),
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
    "GET  /api/custodians",
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
    "GET  /api/servicing/topics",
    "GET  /api/servicing/jobs",
    "GET  /api/communities/models?q=...",
    "POST /api/communities/compare",
    "GET  /api/audit",
    "GET  /api/accounts/{id}/notes",
    "POST /api/accounts/{id}/notes",
    "GET  /api/roles/{role}/capabilities",
    "POST /api/prospect",
    "GET  /api/strategies",
    "POST /api/strategies                   (create)",
    "GET  /api/households/{id}/allocations",
    "POST /api/households/{id}/allocations  (fund a managed allocation)",
    "GET  /api/allocations/{id}/decisions",
    "POST /api/allocations/{id}/tick        (run one agent loop iteration)",
    "POST /api/allocations/{id}/active      (kill switch)",
    "POST /api/allocations/{id}/self-improve (propose a refinement)",
    "GET  /api/allocations/{id}/improvements",
    "POST /api/improvements/{id}/resolve    (human approve/reject)",
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
    async fn allocation_lifecycle_fund_tick_decisions_killswitch() {
        let c = client().await;
        let hs: Vec<HouseholdDtoOwned> = c.get("/api/households").await.json();
        let hid = &hs[0].id;

        // fund a managed allocation
        let created: serde_json::Value = c
            .request(http::Method::POST, &format!("/api/households/{hid}/allocations"))
            .json(&serde_json::json!({"name": "Growth", "mandate": "growth", "risk_level": 4, "funded": 100000.0}))
            .send()
            .await
            .json();
        let aid = created["id"].as_str().unwrap().to_string();

        let allocs: Vec<serde_json::Value> = c
            .get(&format!("/api/households/{hid}/allocations"))
            .await
            .json();
        assert!(allocs.iter().any(|a| a["id"] == aid && a["funded"] == 100000.0 && a["active"] == true));

        // run one loop tick: a safe-zone candidate should fill
        let results: Vec<serde_json::Value> = c
            .request(http::Method::POST, &format!("/api/allocations/{aid}/tick"))
            .json(&serde_json::json!({"candidates": ["AAPL"], "zones": {"AAPL": "safe"}, "quotes": "AAPL:200"}))
            .send()
            .await
            .json();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["filled"], true);

        // a decision was logged
        let decisions: Vec<serde_json::Value> = c.get(&format!("/api/allocations/{aid}/decisions")).await.json();
        assert!(decisions.iter().any(|d| d["admitted"] == true && d["ticker"] == "AAPL"));

        // kill switch halts trading
        let _: serde_json::Value = c
            .request(http::Method::POST, &format!("/api/allocations/{aid}/active"))
            .json(&serde_json::json!({"active": false}))
            .send()
            .await
            .json();
        let halted: Vec<serde_json::Value> = c
            .request(http::Method::POST, &format!("/api/allocations/{aid}/tick"))
            .json(&serde_json::json!({"candidates": ["MSFT"], "zones": {"MSFT": "safe"}, "quotes": "MSFT:400"}))
            .send()
            .await
            .json();
        assert!(halted.is_empty(), "halted allocation must not trade");
    }

    #[tokio::test]
    async fn strategy_endpoint_create_and_list() {
        let c = client().await;
        let created: serde_json::Value = c
            .request(http::Method::POST, "/api/strategies")
            .json(&serde_json::json!({"name": "Momentum Rotation", "kind": "rotation", "description": "rotate sector ETFs"}))
            .send()
            .await
            .json();
        assert!(created["id"].is_string());
        let strategies: Vec<serde_json::Value> = c.get("/api/strategies").await.json();
        assert!(strategies.iter().any(|s| s["name"] == "Momentum Rotation"));
    }

    #[tokio::test]
    async fn custodians_listed_and_account_carries_one() {
        let c = client().await;
        let custodians: Vec<serde_json::Value> = c.get("/api/custodians").await.json();
        // seed registers Schwab/Fidelity/Pershing/Altruist
        assert!(custodians.iter().any(|x| x["name"] == "Schwab"));
        assert!(custodians.iter().any(|x| x["name"] == "Fidelity"));

        // seeded accounts custody at Schwab
        let hs: Vec<HouseholdDtoOwned> = c.get("/api/households").await.json();
        let accts: Vec<serde_json::Value> = c
            .get(&format!("/api/households/{}/accounts", hs[0].id))
            .await
            .json();
        assert!(accts.iter().all(|a| a["custodian"] == "Schwab"));
    }

    #[tokio::test]
    async fn create_account_with_custodian_name_get_or_creates() {
        let c = client().await;
        let created: serde_json::Value = c
            .request(http::Method::POST, "/api/households")
            .json(&serde_json::json!({"name": "Custody Test"}))
            .send()
            .await
            .json();
        let hid = created["id"].as_str().unwrap().to_string();

        // create with a brand-new custodian name → it should be created and attached
        let acc: serde_json::Value = c
            .request(http::Method::POST, &format!("/api/households/{hid}/accounts"))
            .json(&serde_json::json!({"number": "INDV-CT-001", "account_type": "Individual", "cash": 0.0, "custodian": "LPL"}))
            .send()
            .await
            .json();
        assert!(acc["id"].is_string());

        let accts: Vec<serde_json::Value> = c
            .get(&format!("/api/households/{hid}/accounts"))
            .await
            .json();
        assert_eq!(accts[0]["custodian"], "LPL");
        // and LPL now shows in the reference list
        let custodians: Vec<serde_json::Value> = c.get("/api/custodians").await.json();
        assert!(custodians.iter().any(|x| x["name"] == "LPL"));
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

    #[tokio::test]
    async fn servicing_and_communities_endpoints() {
        let c = client().await;
        let topics: Vec<String> = c.get("/api/servicing/topics").await.json();
        assert!(topics.iter().any(|t| t == "Address Change"));
        let jobs: Vec<serde_json::Value> = c.get("/api/servicing/jobs").await.json();
        assert!(jobs.iter().any(|j| j["name"] == "TLH scan"));
        let models: Vec<serde_json::Value> = c.get("/api/communities/models?q=esg").await.json();
        assert_eq!(models.len(), 1);
        let all: Vec<serde_json::Value> = c.get("/api/communities/models?q=").await.json();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn audit_endpoint_returns_events() {
        let c = client().await;
        // the seed writes a "seeded household" audit event
        let events: Vec<serde_json::Value> = c.get("/api/audit").await.json();
        assert!(events.iter().any(|e| e["entity"] == "household"));
        // a create writes another
        let _ = c
            .request(http::Method::POST, "/api/households")
            .json(&serde_json::json!({"name": "Audit Test"}))
            .send()
            .await;
        let after: Vec<serde_json::Value> = c.get("/api/audit").await.json();
        assert!(after.len() > events.len());
        // the new household's create event is present (ordering within a 1s tick is not asserted).
        assert!(
            after
                .iter()
                .any(|e| e["action"] == "created" && e["detail"] == "Audit Test")
        );
    }

    #[tokio::test]
    async fn account_notes_roundtrip() {
        let c = client().await;
        let hs: Vec<HouseholdDtoOwned> = c.get("/api/households").await.json();
        let accts: Vec<serde_json::Value> = c
            .get(&format!("/api/households/{}/accounts", hs[0].id))
            .await
            .json();
        let aid = accts[0]["id"].as_str().unwrap().to_string();

        let empty: Vec<serde_json::Value> =
            c.get(&format!("/api/accounts/{aid}/notes")).await.json();
        assert_eq!(empty.len(), 0);

        let r = c
            .request(http::Method::POST, &format!("/api/accounts/{aid}/notes"))
            .json(&serde_json::json!({"body": "client wants more bonds next quarter"}))
            .send()
            .await;
        assert_eq!(r.status(), 200);

        let notes: Vec<serde_json::Value> =
            c.get(&format!("/api/accounts/{aid}/notes")).await.json();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0]["body"], "client wants more bonds next quarter");
    }

    #[tokio::test]
    async fn role_capabilities_endpoint() {
        let c = client().await;
        let caps: Vec<serde_json::Value> = c.get("/api/roles/User/capabilities").await.json();
        assert_eq!(caps.len(), 7);
        assert!(
            caps.iter()
                .any(|x| x["capability"] == "ManageBilling" && x["allowed"] == false)
        );
        let admin: Vec<serde_json::Value> = c.get("/api/roles/FirmAdmin/capabilities").await.json();
        assert!(
            admin
                .iter()
                .any(|x| x["capability"] == "ManageBilling" && x["allowed"] == true)
        );
    }

    #[tokio::test]
    async fn prospect_endpoint() {
        let c = client().await;
        let r = c
            .request(http::Method::POST, "/api/prospect")
            .json(&serde_json::json!({"investable": 100000.0, "risk": 2}))
            .send()
            .await;
        assert_eq!(r.status(), 200);
        let fits: Vec<serde_json::Value> = r.json();
        assert!(!fits.is_empty());
        assert_eq!(fits[0]["eligible"], true); // best fit is eligible
    }

    // Owned mirror of HouseholdDto for deserializing in tests.
    #[derive(serde::Deserialize)]
    struct HouseholdDtoOwned {
        id: String,
        name: String,
    }
}
