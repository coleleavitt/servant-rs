//! ponoma-mcp — a real MCP server (rmcp SDK, stdio transport) exposing ponoma's tool catalog
//! over the model-context protocol, like Orion's oat-mcp-server but on ponoma's own book. It
//! delegates every `tools/call` to `ponoma_server::mcp::call_tool`, so the deterministic risk
//! gate still governs any action tool. Run: `cargo run -p ponoma-server --bin ponoma-mcp`.

use std::sync::Arc;

use rmcp::{
    model::*,
    service::{RequestContext, RoleServer},
    transport::stdio,
    ServerHandler, ServiceExt,
};

use ponoma_server::{bootstrap, mcp, Db};

#[derive(Clone)]
struct PonomaMcp {
    db: Db,
}

impl ServerHandler for PonomaMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Ponoma MCP — multi-household book of record + paper trading. Tools read the book \
             (households/accounts/holdings/AUM), preview rebalances, compute billing, and \
             paper-trade through a hard risk gate.",
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = mcp::list_tools()
            .into_iter()
            .map(|t| {
                let props: serde_json::Map<String, serde_json::Value> = t
                    .params
                    .iter()
                    .map(|p| (p.to_string(), serde_json::json!({ "type": "string" })))
                    .collect();
                let schema = serde_json::json!({ "type": "object", "properties": props });
                Tool::new(t.name, t.description, Arc::new(serde_json::from_value(schema).unwrap()))
            })
            .collect();
        Ok(ListToolsResult { tools, meta: None, next_cursor: None })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let args = request
            .arguments
            .map(serde_json::Value::Object)
            .unwrap_or(serde_json::Value::Null);
        match mcp::call_tool(&self.db, request.name.as_ref(), &args).await {
            Ok(v) => Ok(CallToolResult::success(vec![ContentBlock::text(
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
            )])),
            Err(e) => Err(ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("PONOMA_DB").unwrap_or_else(|_| "sqlite://ponoma.db?mode=rwc".into());
    let db = bootstrap(&url).await?;
    let service = PonomaMcp { db }.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
