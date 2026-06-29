//! The agentic operating model (Phase 9 / CONCEPT.md): managed allocations, strategies, the
//! decision log, and the autonomous loop tick.
//!
//! A **managed allocation** is a slice of cash an agent runs as a local paper portfolio. It owns
//! a dedicated paper `account` (so the existing [`crate::paper`] engine + deterministic risk gate
//! apply unchanged), a mandate, and a HARD per-allocation risk envelope the agent cannot widen.
//!
//! A **strategy** is a first-class trading system (a versioned policy) assignable to an allocation.
//!
//! The **loop tick** is one deterministic `watch → understand → decide → gate → act` iteration:
//! a thesis (from the distress zone) becomes a proposal, the deterministic gate admits/rejects it,
//! and on admission a paper fill posts to the allocation's book. Every step is logged. The
//! per-allocation `active` flag is the kill switch — a halted allocation never trades.

use sqlx::Row;
use uuid::Uuid;

use crate::db::{Db, DbError};
use crate::domain::{Quotes, TradeAction};
use crate::paper::{PaperError, RiskLimits};
use crate::thesis::{self, Verdict, Zone};

#[derive(Clone, Debug, PartialEq)]
pub struct Strategy {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub model_id: Option<String>,
    pub rules: String,
    pub version: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManagedAllocation {
    pub id: String,
    pub household_id: String,
    pub paper_account_id: String,
    pub source_account_id: Option<String>,
    pub strategy_id: Option<String>,
    pub name: String,
    pub mandate: String,
    pub risk_level: i64,
    pub funded: f64,
    pub max_order_frac: f64,
    pub max_cash_use_frac: f64,
    pub allow_short: bool,
    pub active: bool,
}

impl ManagedAllocation {
    /// The hard risk envelope for this allocation (mirrors [`RiskLimits`]). The agent loop runs
    /// every proposal through this — it can never widen it.
    pub fn limits(&self) -> RiskLimits {
        RiskLimits {
            max_order_frac: self.max_order_frac,
            allow_short: self.allow_short,
            max_cash_use_frac: self.max_cash_use_frac,
        }
    }
}

/// The decision policy a strategy imposes on the loop tick, parsed from `strategy.kind` + the
/// `rules` JSON. Deterministic; absent a strategy the base distress-thesis policy is used.
#[derive(Clone, Debug, Default)]
pub struct StrategyPolicy {
    pub kind: String,
    /// SELL any name whose distress Z″ proxy is below this (rules: `sellWhenZBelow`).
    pub sell_when_z_below: Option<f64>,
    /// cap on the number of distinct holdings the allocation may carry (rules: `maxNames`).
    pub max_names: Option<usize>,
    /// only BUYs are allowed (kind = follow-disclosures / rotation buy-side).
    pub buy_only: bool,
    /// only SELLs are allowed (kind = tactical-derisk / distress-avoid).
    pub sell_only: bool,
}

impl StrategyPolicy {
    fn from_strategy(s: &Strategy) -> Self {
        let v: serde_json::Value = serde_json::from_str(&s.rules).unwrap_or(serde_json::Value::Null);
        let sell_when_z_below = v.get("sellWhenZBelow").and_then(|x| x.as_f64());
        let max_names = v
            .get("maxNames")
            .and_then(|x| x.as_u64())
            .map(|n| n as usize);
        // kind-implied side bias (rules can't widen the envelope; this only narrows actions).
        let (buy_only, sell_only) = match s.kind.as_str() {
            "distress-avoid" | "tactical-derisk" => (false, true),
            "follow-disclosures" => (true, false),
            _ => (
                v.get("buyOnly").and_then(|x| x.as_bool()).unwrap_or(false),
                v.get("sellOnly").and_then(|x| x.as_bool()).unwrap_or(false),
            ),
        };
        Self {
            kind: s.kind.clone(),
            sell_when_z_below,
            max_names,
            buy_only,
            sell_only,
        }
    }

    /// Apply the policy to a base action for one candidate. Returns the (possibly overridden)
    /// action, or `None` to force HOLD with a reason. `zone` is the distress zone; `held_names`
    /// is the current distinct-holding count (for the maxNames cap).
    fn decide(
        &self,
        base: Option<TradeAction>,
        zone: Zone,
        held_names: usize,
    ) -> (Option<TradeAction>, Option<&'static str>) {
        // distress-avoid override: a distress-zone name is sold regardless of the base thesis.
        if self.sell_when_z_below.is_some() && matches!(zone, Zone::Distress) {
            return (Some(TradeAction::Sell), Some("strategy: distress sell rule"));
        }
        match base {
            Some(TradeAction::Buy) if self.sell_only => (None, Some("strategy: sell-only")),
            Some(TradeAction::Sell) if self.buy_only => (None, Some("strategy: buy-only")),
            Some(TradeAction::Buy)
                if self.max_names.is_some_and(|m| held_names >= m) =>
            {
                (None, Some("strategy: maxNames reached"))
            }
            other => (other, None),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentDecision {
    pub id: String,
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

/// Outcome of one loop tick for one candidate ticker.
#[derive(Clone, Debug, PartialEq)]
pub struct TickResult {
    pub ticker: String,
    pub action: String,
    pub admitted: bool,
    pub filled: bool,
    pub thesis: String,
    pub verdict: String,
}

impl Db {
    // ── strategies ───────────────────────────────────────────────────────────
    pub async fn create_strategy(
        &self,
        name: &str,
        kind: &str,
        description: &str,
        model_id: Option<&str>,
        rules: &str,
    ) -> Result<String, DbError> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO strategy (id, name, kind, description, model_id, rules) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id).bind(name).bind(kind).bind(description).bind(model_id).bind(rules)
        .execute(&self.pool).await?;
        self.audit(None, "created", "strategy", name).await?;
        Ok(id)
    }

    pub async fn strategies(&self) -> Result<Vec<Strategy>, DbError> {
        let rows = sqlx::query(
            "SELECT id, name, kind, description, model_id, rules, version FROM strategy ORDER BY created_at DESC, name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| Strategy {
                id: r.get("id"),
                name: r.get("name"),
                kind: r.get("kind"),
                description: r.get("description"),
                model_id: r.try_get("model_id").ok(),
                rules: r.get("rules"),
                version: r.get("version"),
            })
            .collect())
    }

    // ── managed allocations ──────────────────────────────────────────────────
    /// Fund a new managed allocation: create its dedicated paper account (seeded with `funded`
    /// cash, drawn on paper from `source_account_id` if given) and the allocation record.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_allocation(
        &self,
        household_id: &str,
        name: &str,
        mandate: &str,
        risk_level: i64,
        funded: f64,
        source_account_id: Option<&str>,
        strategy_id: Option<&str>,
    ) -> Result<String, DbError> {
        // dedicated paper book for this allocation
        let paper_account_id = self
            .create_account(household_id, &format!("ALLOC-{name}"), "Managed Allocation", funded, None)
            .await?;

        // draw the cash from the source account on paper (so the book balances), best-effort.
        if let Some(src) = source_account_id {
            sqlx::query("UPDATE account SET cash = cash - ? WHERE id = ?")
                .bind(funded)
                .bind(src)
                .execute(&self.pool)
                .await?;
        }

        let id = Uuid::new_v4().to_string();
        let lim = RiskLimits::default();
        sqlx::query(
            "INSERT INTO managed_allocation \
             (id, household_id, source_account_id, paper_account_id, strategy_id, name, mandate, \
              risk_level, funded, max_order_frac, max_cash_use_frac, allow_short, active) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 1)",
        )
        .bind(&id)
        .bind(household_id)
        .bind(source_account_id)
        .bind(&paper_account_id)
        .bind(strategy_id)
        .bind(name)
        .bind(mandate)
        .bind(risk_level)
        .bind(funded)
        .bind(lim.max_order_frac)
        .bind(lim.max_cash_use_frac)
        .execute(&self.pool)
        .await?;
        self.audit(Some(household_id), "created", "allocation", name).await?;
        Ok(id)
    }

    pub async fn allocations_for_household(
        &self,
        household_id: &str,
    ) -> Result<Vec<ManagedAllocation>, DbError> {
        let rows = sqlx::query(
            "SELECT id, household_id, source_account_id, paper_account_id, strategy_id, name, \
                    mandate, risk_level, funded, max_order_frac, max_cash_use_frac, allow_short, active \
             FROM managed_allocation WHERE household_id = ? ORDER BY created_at DESC",
        )
        .bind(household_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_allocation).collect())
    }

    pub async fn allocation(&self, id: &str) -> Result<Option<ManagedAllocation>, DbError> {
        let row = sqlx::query(
            "SELECT id, household_id, source_account_id, paper_account_id, strategy_id, name, \
                    mandate, risk_level, funded, max_order_frac, max_cash_use_frac, allow_short, active \
             FROM managed_allocation WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_allocation))
    }

    /// The kill switch: set an allocation active/halted. A halted allocation never trades.
    pub async fn set_allocation_active(&self, id: &str, active: bool) -> Result<(), DbError> {
        sqlx::query("UPDATE managed_allocation SET active = ? WHERE id = ?")
            .bind(active as i64)
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.audit(None, if active { "resumed" } else { "halted" }, "allocation", id)
            .await?;
        Ok(())
    }

    // ── decision log ─────────────────────────────────────────────────────────
    #[allow(clippy::too_many_arguments)]
    async fn log_decision(
        &self,
        allocation_id: &str,
        step: &str,
        ticker: &str,
        action: &str,
        shares: f64,
        thesis: &str,
        confidence: f64,
        admitted: bool,
        verdict: &str,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO agent_decision \
             (id, allocation_id, step, ticker, action, shares, thesis, confidence, admitted, verdict) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(allocation_id)
        .bind(step)
        .bind(ticker)
        .bind(action)
        .bind(shares)
        .bind(thesis)
        .bind(confidence)
        .bind(admitted as i64)
        .bind(verdict)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn decisions_for_allocation(
        &self,
        allocation_id: &str,
        limit: i64,
    ) -> Result<Vec<AgentDecision>, DbError> {
        let rows = sqlx::query(
            "SELECT id, step, ticker, action, shares, thesis, confidence, admitted, verdict, at \
             FROM agent_decision WHERE allocation_id = ? ORDER BY at DESC, id DESC LIMIT ?",
        )
        .bind(allocation_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| AgentDecision {
                id: r.get("id"),
                step: r.get("step"),
                ticker: r.get("ticker"),
                action: r.get("action"),
                shares: r.get("shares"),
                thesis: r.get("thesis"),
                confidence: r.get("confidence"),
                admitted: r.get::<i64, _>("admitted") != 0,
                verdict: r.get("verdict"),
                at: r.get("at"),
            })
            .collect())
    }

    /// Count of distinct tickers currently held (shares > 0) in a paper book — for maxNames.
    async fn distinct_holdings(&self, account_id: &str) -> Result<usize, DbError> {
        let n: i64 = sqlx::query(
            "SELECT COUNT(DISTINCT ticker) AS n FROM holding WHERE account_id = ? AND shares > 0",
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await?
        .get("n");
        Ok(n as usize)
    }

     // ── model-aware rebalance ────────────────────────────────────────────────
     /// Rebalance a managed allocation toward its model's target weights.
     /// For each target ticker, computes delta$ = target$ − current$, then BUYs or SELLs to close
     /// that gap. Every trade passes through the risk gate. A halted allocation (kill switch off)
     /// does nothing. Returns TickResults for each target ticker. Respects strategy policy
     /// (buy-only, sell-only, etc.).
     pub async fn rebalance_to_model(
         &self,
         allocation_id: &str,
         quotes: &Quotes,
     ) -> Result<Vec<TickResult>, PaperError> {
         let alloc = self
             .allocation(allocation_id)
             .await?
             .ok_or(PaperError::AccountNotFound)?;
         if !alloc.active {
             // kill switch engaged — log and return empty.
             self.log_decision(allocation_id, "rebalance", "", "", 0.0, "allocation halted (kill switch)", 0.0, false, "halted")
                 .await?;
             return Ok(Vec::new());
         }

         // Load the allocation's model (via its strategy).
         let model_id = self.allocation_model_id(allocation_id).await?;
         let Some(mid) = model_id else {
             // No model assigned — log and return nothing.
             return Ok(vec![TickResult {
                 ticker: "".into(),
                 action: "HOLD".into(),
                 admitted: false,
                 filled: false,
                 thesis: "no model assigned to allocation".into(),
                 verdict: "no model".into(),
             }]);
         };

         let limits = alloc.limits();
         let target_holdings = self.model_targets(&mid).await?;
         let val = self.value_account(&alloc.paper_account_id, quotes).await?;
         let total_value = val.total_value;

         // Load current positions for held tickers (for delta sizing).
         let current_positions = self.positions_for_account(&alloc.paper_account_id).await?;
         let mut current_by_ticker: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
         for pos in current_positions {
             current_by_ticker.insert(pos.ticker.to_uppercase(), pos.shares);
         }

         // Load strategy policy (if any) to check buy-only / sell-only constraints.
         let policy = match &alloc.strategy_id {
             Some(sid) => self
                 .strategies()
                 .await?
                 .into_iter()
                 .find(|s| &s.id == sid)
                 .map(|s| StrategyPolicy::from_strategy(&s)),
             None => None,
         };

         let mut results = Vec::new();
         let churn_threshold = (total_value * 0.005).max(1.0); // min 0.5% of value or 1 share price unit

         for (ticker, target_wt_pct) in target_holdings {
             let t = ticker.to_uppercase();
             
             // Get current price.
             let Some(&price) = quotes.get(&t) else {
                 self.log_decision(allocation_id, "rebalance", &t, "HOLD", 0.0, &format!("rebalance toward {target_wt_pct:.1}%"), 0.0, false, "no quote")
                     .await?;
                 results.push(TickResult {
                     ticker: t,
                     action: "HOLD".into(),
                     admitted: false,
                     filled: false,
                     thesis: "no quote available".into(),
                     verdict: "no quote".into(),
                 });
                 continue;
             };

             // Compute target notional and current notional.
             let target_notional = (target_wt_pct / 100.0) * total_value;
             let current_shares = current_by_ticker.get(&t).copied().unwrap_or(0.0);
             let current_notional = current_shares * price;
             let delta_notional = target_notional - current_notional;

             // Skip trivial deltas (churn threshold).
             if delta_notional.abs() < churn_threshold {
                 self.log_decision(
                     allocation_id,
                     "rebalance",
                     &t,
                     "HOLD",
                     0.0,
                     &format!("rebalance toward {target_wt_pct:.1}%; delta ${delta_notional:.2} < threshold ${churn_threshold:.2}"),
                     0.0,
                     false,
                     "below churn threshold",
                 )
                 .await?;
                 results.push(TickResult {
                     ticker: t,
                     action: "HOLD".into(),
                     admitted: false,
                     filled: false,
                     thesis: "delta below churn threshold".into(),
                     verdict: "hold".into(),
                 });
                 continue;
             }

             // Determine action and shares to trade.
             let (action, shares_to_trade) = if delta_notional > 0.0 {
                 // BUY to reach target
                 (TradeAction::Buy, (delta_notional / price).floor())
             } else {
                 // SELL to reach target
                 (TradeAction::Sell, (-delta_notional / price).floor().min(current_shares))
             };

             if shares_to_trade <= 0.0 {
                 self.log_decision(
                     allocation_id,
                     "rebalance",
                     &t,
                     match action {
                         TradeAction::Buy => "BUY",
                         TradeAction::Sell => "SELL",
                     },
                     0.0,
                     &format!("rebalance toward {target_wt_pct:.1}%"),
                     0.0,
                     false,
                     "size below one share",
                 )
                 .await?;
                 let act = match action {
                     TradeAction::Buy => "BUY",
                     TradeAction::Sell => "SELL",
                 };
                 results.push(TickResult {
                     ticker: t,
                     action: act.into(),
                     admitted: false,
                     filled: false,
                     thesis: "delta < one share".into(),
                     verdict: "too small".into(),
                 });
                 continue;
             }

              // Apply strategy policy constraints.
              let (allowed_action, rationale) = if let Some(ref p) = policy {
                 let (opt_act, reason) = match action {
                     TradeAction::Buy if p.buy_only || !p.sell_only => (Some(action), None),
                     TradeAction::Buy if p.sell_only => (None, Some("strategy: sell-only")),
                     TradeAction::Sell if p.sell_only || !p.buy_only => (Some(action), None),
                     TradeAction::Sell if p.buy_only => (None, Some("strategy: buy-only")),
                     _ => (Some(action), None),
                 };
                 (opt_act, reason.map(|r| r.to_string()).unwrap_or_default())
             } else {
                 (Some(action), String::new())
             };

              let Some(final_action) = allowed_action else {
                  let act = match action {
                      TradeAction::Buy => "BUY",
                      TradeAction::Sell => "SELL",
                  };
                  let thesis_str = format!("rebalance toward {target_wt_pct:.1}%; {rationale}");
                  self.log_decision(
                      allocation_id,
                      "rebalance",
                      &t,
                      act,
                      shares_to_trade,
                      &thesis_str,
                      0.0,
                      false,
                      &rationale,
                  )
                  .await?;
                  results.push(TickResult {
                      ticker: t,
                      action: act.into(),
                      admitted: false,
                      filled: false,
                      thesis: thesis_str,
                      verdict: rationale,
                  });
                  continue;
              };

             let act_str = match final_action {
                 TradeAction::Buy => "BUY",
                 TradeAction::Sell => "SELL",
             };

             // Gate + act: run through the risk gate.
             match self.paper_execute(&alloc.paper_account_id, final_action, &t, shares_to_trade, price, &limits).await {
                 Ok(_) => {
                     self.log_decision(
                         allocation_id,
                         "rebalance",
                         &t,
                         act_str,
                         shares_to_trade,
                         &format!("rebalance toward {target_wt_pct:.1}%"),
                         1.0,
                         true,
                         "filled (paper)",
                     )
                     .await?;
                     results.push(TickResult {
                         ticker: t,
                         action: act_str.into(),
                         admitted: true,
                         filled: true,
                         thesis: format!("rebalance toward {target_wt_pct:.1}%"),
                         verdict: "filled".into(),
                     });
                 }
                 Err(PaperError::RiskRejected(why)) => {
                     self.log_decision(
                         allocation_id,
                         "rebalance",
                         &t,
                         act_str,
                         shares_to_trade,
                         &format!("rebalance toward {target_wt_pct:.1}%"),
                         1.0,
                         false,
                         &why,
                     )
                     .await?;
                     results.push(TickResult {
                         ticker: t,
                         action: act_str.into(),
                         admitted: false,
                         filled: false,
                         thesis: format!("rebalance toward {target_wt_pct:.1}%"),
                         verdict: why,
                     });
                 }
                 Err(e) => return Err(e),
             }
         }

         Ok(results)
     }

     // ── the autonomous loop tick ──────────────────────────────────────────────
     /// Run ONE deterministic loop iteration over `candidates`: for each ticker, synthesize a
     /// thesis from its distress `zone`, decide an action, run the deterministic risk gate, and on
     /// admission post a paper fill to the allocation's book. Every decision is logged. A halted
     /// allocation (kill switch off) does nothing. `quotes` prices the fills; `zones` supplies the
     /// distress zone per ticker (from the screener). The LLM never executes — it can only feed
     /// candidates/zones; this gate is the only path to a fill.
     pub async fn run_loop_tick(
        &self,
        allocation_id: &str,
        candidates: &[String],
        zones: &std::collections::HashMap<String, Zone>,
        quotes: &Quotes,
    ) -> Result<Vec<TickResult>, PaperError> {
        let alloc = self
            .allocation(allocation_id)
            .await?
            .ok_or(PaperError::AccountNotFound)?;
        if !alloc.active {
            // kill switch engaged — record the no-op and return nothing.
            self.log_decision(allocation_id, "watch", "", "", 0.0, "allocation halted (kill switch)", 0.0, false, "halted")
                .await?;
            return Ok(Vec::new());
        }

        let limits = alloc.limits();
        // Load the assigned strategy's policy (deterministic rule layer over the base thesis).
        let policy = match &alloc.strategy_id {
            Some(sid) => self
                .strategies()
                .await?
                .into_iter()
                .find(|s| &s.id == sid)
                .map(|s| StrategyPolicy::from_strategy(&s)),
            None => None,
        };
        let mut results = Vec::new();

        for ticker in candidates {
            let t = ticker.to_uppercase();
            let zone = zones.get(&t).copied().unwrap_or(Zone::Unknown);
            let thesis = thesis::synthesize(&t, zone, &[]);
            let mut rationale = thesis.rationale.join("; ");

            // decide: Opportunity → BUY, Avoid → SELL (trim), else HOLD.
            let base_action = match thesis.verdict {
                Verdict::Opportunity => Some(TradeAction::Buy),
                Verdict::Avoid => Some(TradeAction::Sell),
                _ => None,
            };
            // apply the strategy policy (if any): it can override to SELL on distress, force a
            // side, or block on the maxNames cap. It can only NARROW actions, never widen risk.
            let action = if let Some(p) = &policy {
                let held_names = self.distinct_holdings(&alloc.paper_account_id).await?;
                let (a, why) = p.decide(base_action, zone, held_names);
                if let Some(reason) = why {
                    rationale = format!("{rationale}; {reason}");
                }
                a
            } else {
                base_action
            };
            let Some(action) = action else {
                self.log_decision(allocation_id, "decide", &t, "HOLD", 0.0, &rationale, thesis.confidence, false, "no actionable edge")
                    .await?;
                results.push(TickResult { ticker: t, action: "HOLD".into(), admitted: false, filled: false, thesis: rationale, verdict: "hold".into() });
                continue;
            };
            let act_str = match action {
                TradeAction::Buy => "BUY",
                TradeAction::Sell => "SELL",
            };

            let Some(&price) = quotes.get(&t) else {
                self.log_decision(allocation_id, "gate", &t, act_str, 0.0, &rationale, thesis.confidence, false, "no quote")
                    .await?;
                results.push(TickResult { ticker: t, action: act_str.into(), admitted: false, filled: false, thesis: rationale, verdict: "no quote".into() });
                continue;
            };

            // size: a single envelope-bounded clip — max_order_frac of the book value.
            let val = self.value_account(&alloc.paper_account_id, quotes).await?;
            let budget = limits.max_order_frac * val.total_value;
            let shares = (budget / price).floor().max(0.0);
            if shares <= 0.0 {
                self.log_decision(allocation_id, "gate", &t, act_str, 0.0, &rationale, thesis.confidence, false, "size below one share")
                    .await?;
                results.push(TickResult { ticker: t, action: act_str.into(), admitted: false, filled: false, thesis: rationale, verdict: "too small".into() });
                continue;
            }

            // gate + act: the existing deterministic paper engine enforces the envelope.
            match self.paper_execute(&alloc.paper_account_id, action, &t, shares, price, &limits).await {
                Ok(_) => {
                    self.log_decision(allocation_id, "act", &t, act_str, shares, &rationale, thesis.confidence, true, "filled (paper)")
                        .await?;
                    results.push(TickResult { ticker: t, action: act_str.into(), admitted: true, filled: true, thesis: rationale, verdict: "filled".into() });
                }
                Err(PaperError::RiskRejected(why)) => {
                    self.log_decision(allocation_id, "gate", &t, act_str, shares, &rationale, thesis.confidence, false, &why)
                        .await?;
                    results.push(TickResult { ticker: t, action: act_str.into(), admitted: false, filled: false, thesis: rationale, verdict: why });
                }
                Err(e) => return Err(e),
            }
        }
        Ok(results)
    }
}

fn row_to_allocation(r: sqlx::sqlite::SqliteRow) -> ManagedAllocation {
    ManagedAllocation {
        id: r.get("id"),
        household_id: r.get("household_id"),
        source_account_id: r.try_get("source_account_id").ok(),
        paper_account_id: r.get("paper_account_id"),
        strategy_id: r.try_get("strategy_id").ok(),
        name: r.get("name"),
        mandate: r.get("mandate"),
        risk_level: r.get("risk_level"),
        funded: r.get("funded"),
        max_order_frac: r.get("max_order_frac"),
        max_cash_use_frac: r.get("max_cash_use_frac"),
        allow_short: r.get::<i64, _>("allow_short") != 0,
        active: r.get::<i64, _>("active") != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap;
    use std::collections::HashMap;

    #[tokio::test]
    async fn allocation_funds_a_paper_book_and_runs_a_gated_loop_tick() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        let hh = &db.households().await.unwrap()[0].id;

        let alloc_id = db
            .create_allocation(hh, "Growth", "growth, no tobacco", 4, 100_000.0, None, None)
            .await
            .unwrap();
        let alloc = db.allocation(&alloc_id).await.unwrap().unwrap();
        assert_eq!(alloc.funded, 100_000.0);
        assert!(alloc.active);

        // a BUY candidate in the safe zone → thesis Opportunity → gated paper fill
        let zones = HashMap::from([("AAPL".to_string(), Zone::Safe)]);
        let quotes: Quotes = [("AAPL".to_string(), 200.0)].into_iter().collect();
        let results = db
            .run_loop_tick(&alloc_id, &["AAPL".to_string()], &zones, &quotes)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].filled, "safe-zone opportunity should fill: {:?}", results[0]);

        // a decision was logged and a holding now exists in the allocation's book
        let decisions = db.decisions_for_allocation(&alloc_id, 10).await.unwrap();
        assert!(decisions.iter().any(|d| d.admitted && d.ticker == "AAPL"));
        let valued = db.value_account(&alloc.paper_account_id, &quotes).await.unwrap();
        assert!(valued.total_value > 0.0);
    }

    #[tokio::test]
    async fn kill_switch_halts_all_trading() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        let hh = &db.households().await.unwrap()[0].id;
        let alloc_id = db
            .create_allocation(hh, "Halted", "test", 3, 50_000.0, None, None)
            .await
            .unwrap();
        db.set_allocation_active(&alloc_id, false).await.unwrap();

        let zones = HashMap::from([("AAPL".to_string(), Zone::Safe)]);
        let quotes: Quotes = [("AAPL".to_string(), 200.0)].into_iter().collect();
        let results = db
            .run_loop_tick(&alloc_id, &["AAPL".to_string()], &zones, &quotes)
            .await
            .unwrap();
        assert!(results.is_empty(), "halted allocation must not trade");
    }

    #[tokio::test]
    async fn strategy_create_and_list() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        let id = db
            .create_strategy("Distress Avoid", "distress-avoid", "sell on Z<1.8", None, r#"{"sellWhenZBelow":1.8}"#)
            .await
            .unwrap();
        let all = db.strategies().await.unwrap();
        assert!(all.iter().any(|s| s.id == id && s.name == "Distress Avoid"));
    }

     #[tokio::test]
     async fn strategy_policy_overrides_the_tick() {
         // a distress-avoid (sell-only) strategy must block a safe-zone BUY.
         let db = bootstrap("sqlite::memory:").await.unwrap();
         let hh = &db.households().await.unwrap()[0].id;
         let sid = db
             .create_strategy("Defensive", "distress-avoid", "sell-only", None, r#"{"sellWhenZBelow":1.8}"#)
             .await
             .unwrap();
         let alloc_id = db
             .create_allocation(hh, "Defensive Alloc", "capital preservation", 2, 100_000.0, None, Some(&sid))
             .await
             .unwrap();

         let zones = HashMap::from([("AAPL".to_string(), Zone::Safe)]);
         let quotes: Quotes = [("AAPL".to_string(), 200.0)].into_iter().collect();
         let results = db
             .run_loop_tick(&alloc_id, &["AAPL".to_string()], &zones, &quotes)
             .await
             .unwrap();
         // base thesis says BUY (safe → opportunity), but the sell-only policy forces HOLD.
         assert_eq!(results[0].action, "HOLD");
         assert!(!results[0].filled);
         let decisions = db.decisions_for_allocation(&alloc_id, 10).await.unwrap();
         assert!(decisions.iter().any(|d| d.thesis.contains("sell-only")));

         // and a distress-zone name is SOLD per the rule (after we give it something to sell):
         // distress override fires even though there's nothing held — it proposes SELL, which the
         // gate then rejects (no shares), proving the rule path runs.
         let zones2 = HashMap::from([("XYZ".to_string(), Zone::Distress)]);
         let quotes2: Quotes = [("XYZ".to_string(), 50.0)].into_iter().collect();
         let r2 = db
             .run_loop_tick(&alloc_id, &["XYZ".to_string()], &zones2, &quotes2)
             .await
             .unwrap();
         assert_eq!(r2[0].action, "SELL", "distress rule should propose a SELL");
     }

     #[tokio::test]
     async fn rebalance_to_model_buys_and_sells() {
         let db = bootstrap("sqlite::memory:").await.unwrap();
         let hh = &db.households().await.unwrap()[0].id;

         // Create a model with two holdings: AAPL 10%, MSFT 10% (to fit within 25% max_order_frac).
         let model_id = uuid::Uuid::new_v4().to_string();
         sqlx::query("INSERT INTO model (id, name) VALUES (?, ?)")
             .bind(&model_id)
             .bind("Balanced")
             .execute(&db.pool)
             .await
             .unwrap();
         sqlx::query("INSERT INTO model_holding (model_id, ticker, target_weight) VALUES (?, ?, ?)")
             .bind(&model_id)
             .bind("AAPL")
             .bind(10.0)
             .execute(&db.pool)
             .await
             .unwrap();
         sqlx::query("INSERT INTO model_holding (model_id, ticker, target_weight) VALUES (?, ?, ?)")
             .bind(&model_id)
             .bind("MSFT")
             .bind(10.0)
             .execute(&db.pool)
             .await
             .unwrap();

         // Create a strategy that references this model.
         let sid = db
             .create_strategy("Balanced", "standard", "10/10 balanced", Some(&model_id), "{}")
             .await
             .unwrap();

         // Create an allocation with that strategy, funded with 100k.
         let alloc_id = db
             .create_allocation(hh, "Balanced Alloc", "balanced portfolio", 3, 100_000.0, None, Some(&sid))
             .await
             .unwrap();

         // Rebalance with quotes: AAPL $200, MSFT $400.
         // Target: AAPL $10k (50 sh @ $200), MSFT $10k (25 sh @ $400).
         let quotes: Quotes = [("AAPL".to_string(), 200.0), ("MSFT".to_string(), 400.0)]
             .into_iter()
             .collect();
         let results = db
             .rebalance_to_model(&alloc_id, &quotes)
             .await
             .unwrap();

         // Both AAPL and MSFT should have been BUY'd to reach targets.
         assert_eq!(results.len(), 2);
         let aapl_res = results.iter().find(|r| r.ticker == "AAPL").unwrap();
         let msft_res = results.iter().find(|r| r.ticker == "MSFT").unwrap();
         assert_eq!(aapl_res.action, "BUY");
         assert!(aapl_res.filled, "AAPL should fill: {:?}", aapl_res);
         assert_eq!(msft_res.action, "BUY");
         assert!(msft_res.filled, "MSFT should fill: {:?}", msft_res);

         // Transaction ledger should have 2 entries now.
         let alloc = db.allocation(&alloc_id).await.unwrap().unwrap();
         let txn_count = db.transaction_count(&alloc.paper_account_id).await.unwrap();
         assert_eq!(txn_count, 2, "expected 2 fills in ledger");
     }

     #[tokio::test]
     async fn rebalance_kills_switch_halts() {
         let db = bootstrap("sqlite::memory:").await.unwrap();
         let hh = &db.households().await.unwrap()[0].id;

          // Create a model.
          let model_id = uuid::Uuid::new_v4().to_string();
          sqlx::query("INSERT INTO model (id, name) VALUES (?, ?)")
              .bind(&model_id)
              .bind("Test")
              .execute(&db.pool)
              .await
              .unwrap();
         sqlx::query("INSERT INTO model_holding (model_id, ticker, target_weight) VALUES (?, ?, ?)")
             .bind(&model_id)
             .bind("AAPL")
             .bind(100.0)
             .execute(&db.pool)
             .await
             .unwrap();

         // Create strategy and allocation.
         let sid = db
             .create_strategy("Test", "standard", "test", Some(&model_id), "{}")
             .await
             .unwrap();
         let alloc_id = db
             .create_allocation(hh, "Halted", "test", 2, 50_000.0, None, Some(&sid))
             .await
             .unwrap();

         // Halt the allocation.
         db.set_allocation_active(&alloc_id, false).await.unwrap();

         let quotes: Quotes = [("AAPL".to_string(), 200.0)].into_iter().collect();
         let results = db
             .rebalance_to_model(&alloc_id, &quotes)
             .await
             .unwrap();

          // Halted allocation must return empty (no trades).
          assert!(results.is_empty());

         // Verify no ledger entries.
         let alloc = db.allocation(&alloc_id).await.unwrap().unwrap();
         let txn_count = db.transaction_count(&alloc.paper_account_id).await.unwrap();
         assert_eq!(txn_count, 0);
     }
 }
