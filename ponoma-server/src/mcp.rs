//! MCP tool surface (Phase 7-8, AI parity with oat-mcp-server). Exposes ponoma's capabilities
//! as MCP tools so an agent (or any external MCP client) can drive the book of record. This is
//! the protocol/dispatch layer: `list_tools` advertises the catalog, `call_tool` dispatches to
//! the domain/db/paper/billing layers. Tool *invocation* still runs through the deterministic
//! risk gate (paper.rs) — the agent cannot bypass it.

use crate::billing::{compute_fee, FeeBasis, FeeSchedule, FeeTier};
use crate::db::Db;
use crate::domain::{rebalance_trades, ModelHolding, Quotes, TradeAction};
use crate::paper::RiskLimits;
use serde_json::{json, Value};

/// One MCP tool: name + human description + a JSON-schema-ish parameter hint.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub params: &'static [&'static str],
}

/// The ponoma MCP tool catalog (mirrors oat-mcp-server's shape: read tools + action tools).
pub fn list_tools() -> Vec<Tool> {
    vec![
        Tool { name: "list-households", description: "List all households in the book", params: &[] },
        Tool { name: "household-accounts", description: "List accounts in a household", params: &["household_id"] },
        Tool { name: "account-holdings", description: "Holdings of an account", params: &["account_id"] },
        Tool { name: "account-value", description: "Value an account from quotes", params: &["account_id", "quotes"] },
        Tool { name: "household-aum", description: "Total AUM of a household", params: &["household_id", "quotes"] },
        Tool { name: "rebalance-preview", description: "Preview rebalance trades to a model", params: &["account_id", "model", "quotes"] },
        Tool { name: "billing-calc", description: "Compute the advisory fee on an AUM", params: &["aum", "schedule"] },
        Tool { name: "paper-trade", description: "Paper-execute a trade (through the risk gate)", params: &["account_id", "action", "ticker", "shares", "price"] },
    ]
}

/// JSON-RPC-ish `tools/list` payload (what an MCP client receives).
pub fn tools_list_json() -> Value {
    json!({
        "tools": list_tools().iter().map(|t| json!({
            "name": t.name,
            "description": t.description,
            "inputSchema": { "type": "object", "properties":
                t.params.iter().map(|p| (p.to_string(), json!({"type": "string"}))).collect::<serde_json::Map<_,_>>()
            }
        })).collect::<Vec<_>>()
    })
}

fn quotes_from(v: &Value) -> Quotes {
    v.get("quotes")
        .and_then(|q| q.as_object())
        .map(|m| m.iter().filter_map(|(k, val)| val.as_f64().map(|f| (k.to_uppercase(), f))).collect())
        .unwrap_or_default()
}

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("missing argument: {0}")]
    MissingArg(&'static str),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error(transparent)]
    Paper(#[from] crate::paper::PaperError),
}

/// Dispatch an MCP `tools/call`. Returns a JSON result. Action tools route through the same
/// pure/db/risk layers as the rest of the app.
pub async fn call_tool(db: &Db, name: &str, args: &Value) -> Result<Value, McpError> {
    let arg_str = |k: &'static str| args.get(k).and_then(|v| v.as_str()).map(String::from);
    let arg_f64 = |k: &'static str| args.get(k).and_then(|v| v.as_f64());
    match name {
        "list-households" => {
            let hs = db.households().await?;
            Ok(json!(hs.iter().map(|h| json!({"id": h.id, "name": h.name})).collect::<Vec<_>>()))
        }
        "household-accounts" => {
            let hid = arg_str("household_id").ok_or(McpError::MissingArg("household_id"))?;
            let accts = db.accounts_for_household(&hid).await?;
            Ok(json!(accts.iter().map(|a| json!({"id": a.id, "number": a.number, "type": a.account_type, "cash": a.cash})).collect::<Vec<_>>()))
        }
        "account-holdings" => {
            let aid = arg_str("account_id").ok_or(McpError::MissingArg("account_id"))?;
            let pos = db.positions_for_account(&aid).await?;
            Ok(json!(pos.iter().map(|p| json!({"ticker": p.ticker, "shares": p.shares, "cost_basis": p.cost_basis})).collect::<Vec<_>>()))
        }
        "account-value" => {
            let aid = arg_str("account_id").ok_or(McpError::MissingArg("account_id"))?;
            let v = db.value_account(&aid, &quotes_from(args)).await?;
            Ok(json!({"total_value": v.total_value, "total_gain": v.total_gain}))
        }
        "household-aum" => {
            let hid = arg_str("household_id").ok_or(McpError::MissingArg("household_id"))?;
            Ok(json!({"aum": db.household_aum(&hid, &quotes_from(args)).await?}))
        }
        "rebalance-preview" => {
            let aid = arg_str("account_id").ok_or(McpError::MissingArg("account_id"))?;
            let quotes = quotes_from(args);
            let model: Vec<ModelHolding> = args.get("model").and_then(|m| serde_json::from_value(m.clone()).ok()).unwrap_or_default();
            let valued = db.value_account(&aid, &quotes).await?;
            let trades = rebalance_trades(&valued, &model, &quotes, 1.0);
            Ok(serde_json::to_value(trades).unwrap_or(json!([])))
        }
        "billing-calc" => {
            let aum = arg_f64("aum").ok_or(McpError::MissingArg("aum"))?;
            let schedule = parse_schedule(args.get("schedule"));
            Ok(serde_json::to_value(compute_fee(&schedule, aum)).unwrap_or(json!(null)))
        }
        "paper-trade" => {
            let aid = arg_str("account_id").ok_or(McpError::MissingArg("account_id"))?;
            let action = match arg_str("action").as_deref() { Some("SELL") => TradeAction::Sell, _ => TradeAction::Buy };
            let ticker = arg_str("ticker").ok_or(McpError::MissingArg("ticker"))?;
            let shares = arg_f64("shares").ok_or(McpError::MissingArg("shares"))?;
            let price = arg_f64("price").ok_or(McpError::MissingArg("price"))?;
            let fill = db.paper_execute(&aid, action, &ticker, shares, price, &RiskLimits::default()).await?;
            Ok(json!({"order_id": fill.order_id, "shares": fill.shares, "price": fill.price}))
        }
        other => Err(McpError::UnknownTool(other.to_string())),
    }
}

/// A default standard schedule when the caller doesn't supply one.
fn parse_schedule(v: Option<&Value>) -> FeeSchedule {
    if let Some(s) = v.and_then(|s| serde_json::from_value::<FeeSchedule>(s.clone()).ok()) {
        return s;
    }
    FeeSchedule {
        name: "Standard".into(),
        basis: FeeBasis::Tiered(vec![
            FeeTier { up_to: 1_000_000.0, rate_pct: 1.0 },
            FeeTier { up_to: f64::INFINITY, rate_pct: 0.75 },
        ]),
        minimum: 1_000.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap;

    #[test]
    fn catalog_lists_read_and_action_tools() {
        let tools = list_tools();
        assert!(tools.iter().any(|t| t.name == "list-households"));
        assert!(tools.iter().any(|t| t.name == "paper-trade"));
        let js = tools_list_json();
        assert!(js["tools"].as_array().unwrap().len() >= 8);
    }

    #[tokio::test]
    async fn call_list_households_and_aum() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        let hs = call_tool(&db, "list-households", &json!({})).await.unwrap();
        let hid = hs[0]["id"].as_str().unwrap().to_string();
        let aum = call_tool(&db, "household-aum", &json!({"household_id": hid, "quotes": {"AAPL": 200.0, "MSFT": 400.0}})).await.unwrap();
        assert_eq!(aum["aum"], 19200.0);
    }

    #[tokio::test]
    async fn call_paper_trade_through_gate() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        let hid = db.households().await.unwrap()[0].id.clone();
        let accts = call_tool(&db, "household-accounts", &json!({"household_id": hid})).await.unwrap();
        let aid = accts.as_array().unwrap().iter().find(|a| a["number"] == "INDV-COLE-001").unwrap()["id"].as_str().unwrap().to_string();
        // small admissible trade
        let res = call_tool(&db, "paper-trade", &json!({"account_id": aid, "action": "BUY", "ticker": "VTI", "shares": 2.0, "price": 200.0})).await.unwrap();
        assert_eq!(res["shares"], 2.0);
    }

    #[test]
    fn billing_tool_default_schedule() {
        // not async — call_tool needs a db; test the pure path
        let s = parse_schedule(None);
        assert_eq!(compute_fee(&s, 50_000.0).annual_fee, 1_000.0);
    }
}
