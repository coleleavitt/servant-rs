//! Recursive self-improvement (CONCEPT.md §7), strictly behind the PHILOSOPHY.md rails.
//!
//! The system reads an allocation's **decision-log outcomes** (how its gated paper trades fared)
//! and PROPOSES a refinement to its strategy's rules. It NEVER auto-applies: a proposal is stored
//! as `PROPOSED` and only a human `approve` promotes it into a new strategy **version**. Every
//! step is audited; a halted allocation produces nothing. This is the "improves its own strategy"
//! loop bounded to: paper-only, human-in-the-loop, reversible, fully logged.

use sqlx::Row;
use uuid::Uuid;

use crate::db::{Db, DbError};

#[derive(Clone, Debug, PartialEq)]
pub struct ProposedImprovement {
    pub id: String,
    pub allocation_id: String,
    pub strategy_id: Option<String>,
    pub rationale: String,
    pub current_rules: String,
    pub proposed_rules: String,
    pub status: String,
}

/// A compact read of how an allocation's recent decisions resolved.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OutcomeStats {
    pub total: usize,
    pub admitted: usize,
    pub rejected_size: usize, // rejected for exceeding the order/size envelope
    pub blocked_maxnames: usize, // BUYs blocked by the strategy's maxNames cap
    pub holds: usize,
}

impl OutcomeStats {
    pub fn admit_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.admitted as f64 / self.total as f64
        }
    }
}

impl Db {
    /// Summarize the decision log for an allocation into [`OutcomeStats`].
    pub async fn allocation_outcomes(&self, allocation_id: &str) -> Result<OutcomeStats, DbError> {
        let rows = sqlx::query(
            "SELECT step, action, admitted, verdict, thesis FROM agent_decision WHERE allocation_id = ?",
        )
        .bind(allocation_id)
        .fetch_all(&self.pool)
        .await?;
        let mut s = OutcomeStats::default();
        for r in &rows {
            s.total += 1;
            let admitted: i64 = r.get("admitted");
            let action: String = r.get("action");
            let verdict: String = r.get("verdict");
            let thesis: String = r.get("thesis");
            // strategy-imposed reasons land in the thesis (rationale); gate reasons in the verdict.
            let reasons = format!("{verdict} {thesis}");
            if admitted != 0 {
                s.admitted += 1;
            }
            if action == "HOLD" {
                s.holds += 1;
            }
            if reasons.contains("exceeds") {
                s.rejected_size += 1;
            }
            if reasons.contains("maxNames") {
                s.blocked_maxnames += 1;
            }
        }
        Ok(s)
    }

    /// Propose a rule refinement for an allocation's strategy, learned from its outcomes. Pure,
    /// deterministic heuristic over [`OutcomeStats`]; the result is stored `PROPOSED` (never
    /// applied). Returns `None` when there's nothing to learn yet (no strategy / no decisions).
    pub async fn propose_improvement(
        &self,
        allocation_id: &str,
    ) -> Result<Option<String>, DbError> {
        let Some(alloc) = self.allocation(allocation_id).await? else {
            return Ok(None);
        };
        let Some(strategy_id) = alloc.strategy_id.clone() else {
            return Ok(None);
        };
        let Some(strategy) = self
            .strategies()
            .await?
            .into_iter()
            .find(|s| s.id == strategy_id)
        else {
            return Ok(None);
        };

        let stats = self.allocation_outcomes(allocation_id).await?;
        if stats.total == 0 {
            return Ok(None);
        }

        // Heuristic: if many proposals were rejected for exceeding the size envelope, the strategy
        // is over-sizing — propose a smaller `maxNames` so each clip is smaller / more diversified.
        let mut rules: serde_json::Value =
            serde_json::from_str(&strategy.rules).unwrap_or(serde_json::json!({}));
        let mut rationale = format!(
            "{} decisions: {:.0}% admitted, {} size-rejected, {} maxNames-blocked, {} holds.",
            stats.total,
            stats.admit_rate() * 100.0,
            stats.rejected_size,
            stats.blocked_maxnames,
            stats.holds
        );

        let changed = if stats.blocked_maxnames > 0 {
            // the maxNames cap is binding (BUYs blocked) → raise it to allow more diversification.
            let cur = rules.get("maxNames").and_then(|x| x.as_u64()).unwrap_or(10);
            let next = (cur + 4).min(40);
            rules["maxNames"] = serde_json::json!(next);
            rationale.push_str(&format!(
                " maxNames cap is binding → raise {cur}→{next} to admit more names."
            ));
            true
        } else if stats.rejected_size * 2 >= stats.total.max(1) {
            // ≥50% size-rejected → propose a smaller per-clip footprint via a tighter maxNames.
            let cur = rules.get("maxNames").and_then(|x| x.as_u64()).unwrap_or(10);
            let next = (cur + 4).min(40);
            rules["maxNames"] = serde_json::json!(next);
            rationale.push_str(&format!(
                " High size-rejection → raise maxNames {cur}→{next} for smaller, more diversified clips."
            ));
            true
        } else if stats.holds == stats.total {
            // never acted → loosen by proposing a distress-sell rule it lacks.
            if rules.get("sellWhenZBelow").is_none() {
                rules["sellWhenZBelow"] = serde_json::json!(1.8);
                rationale.push_str(" All holds → add a distress-sell rule (sellWhenZBelow=1.8).");
                true
            } else {
                false
            }
        } else {
            false
        };

        if !changed {
            return Ok(None);
        }

        let id = Uuid::new_v4().to_string();
        let proposed_rules = serde_json::to_string(&rules).unwrap_or_else(|_| "{}".into());
        sqlx::query(
            "INSERT INTO proposed_improvement \
             (id, allocation_id, strategy_id, rationale, current_rules, proposed_rules, status) \
             VALUES (?, ?, ?, ?, ?, ?, 'PROPOSED')",
        )
        .bind(&id)
        .bind(allocation_id)
        .bind(&strategy_id)
        .bind(&rationale)
        .bind(&strategy.rules)
        .bind(&proposed_rules)
        .execute(&self.pool)
        .await?;
        self.audit(None, "proposed", "improvement", &rationale).await?;
        Ok(Some(id))
    }

    pub async fn improvements_for_allocation(
        &self,
        allocation_id: &str,
    ) -> Result<Vec<ProposedImprovement>, DbError> {
        let rows = sqlx::query(
            "SELECT id, allocation_id, strategy_id, rationale, current_rules, proposed_rules, status \
             FROM proposed_improvement WHERE allocation_id = ? ORDER BY created_at DESC",
        )
        .bind(allocation_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ProposedImprovement {
                id: r.get("id"),
                allocation_id: r.get("allocation_id"),
                strategy_id: r.try_get("strategy_id").ok(),
                rationale: r.get("rationale"),
                current_rules: r.get("current_rules"),
                proposed_rules: r.get("proposed_rules"),
                status: r.get("status"),
            })
            .collect())
    }

    /// Human approval: promote a PROPOSED improvement into a new strategy VERSION (applies the
    /// proposed rules + bumps `version`). This is the ONLY path that mutates a strategy from a
    /// proposal — the propose step never does. Rejecting just marks it REJECTED.
    pub async fn resolve_improvement(&self, id: &str, approve: bool) -> Result<(), DbError> {
        let Some(row) = sqlx::query(
            "SELECT strategy_id, proposed_rules, status FROM proposed_improvement WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(());
        };
        let status: String = row.get("status");
        if status != "PROPOSED" {
            return Ok(()); // already resolved — idempotent
        }

        if approve {
            let strategy_id: Option<String> = row.try_get("strategy_id").ok();
            let proposed_rules: String = row.get("proposed_rules");
            if let Some(sid) = strategy_id {
                sqlx::query("UPDATE strategy SET rules = ?, version = version + 1 WHERE id = ?")
                    .bind(&proposed_rules)
                    .bind(&sid)
                    .execute(&self.pool)
                    .await?;
            }
            sqlx::query("UPDATE proposed_improvement SET status = 'APPROVED' WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
            self.audit(None, "approved", "improvement", id).await?;
        } else {
            sqlx::query("UPDATE proposed_improvement SET status = 'REJECTED' WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
            self.audit(None, "rejected", "improvement", id).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap;
    use crate::thesis::Zone;
    use std::collections::HashMap;

    #[tokio::test]
    async fn proposes_then_approval_bumps_strategy_version_nothing_auto_applies() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        let hh = &db.households().await.unwrap()[0].id;
        // a rotation strategy with a tiny maxNames so ticks get size-rejected a lot.
        let sid = db
            .create_strategy("Rotate", "rotation", "", None, r#"{"maxNames":1}"#)
            .await
            .unwrap();
        let aid = db
            .create_allocation(hh, "RSI Test", "growth", 4, 100_000.0, None, Some(&sid))
            .await
            .unwrap();

        // run a few ticks that over-size (25% clips) to generate size-rejections + fills.
        let zones = HashMap::from([
            ("AAPL".to_string(), Zone::Safe),
            ("MSFT".to_string(), Zone::Safe),
            ("NVDA".to_string(), Zone::Safe),
        ]);
        let quotes = [("AAPL".to_string(), 200.0), ("MSFT".to_string(), 400.0), ("NVDA".to_string(), 150.0)]
            .into_iter()
            .collect();
        let cands = vec!["AAPL".to_string(), "MSFT".to_string(), "NVDA".to_string()];
        let _ = db.run_loop_tick(&aid, &cands, &zones, &quotes).await.unwrap();
        let _ = db.run_loop_tick(&aid, &cands, &zones, &quotes).await.unwrap();

        // propose: should learn from the size-rejections and propose a bigger maxNames.
        let prop_id = db.propose_improvement(&aid).await.unwrap();
        assert!(prop_id.is_some(), "should propose an improvement from outcomes");
        let prop_id = prop_id.unwrap();

        // nothing auto-applied: the strategy is still version 1 with maxNames 1.
        let before = db.strategies().await.unwrap().into_iter().find(|s| s.id == sid).unwrap();
        assert_eq!(before.version, 1);
        assert!(before.rules.contains("\"maxNames\":1"));

        // a human approves → new version with the proposed rules.
        db.resolve_improvement(&prop_id, true).await.unwrap();
        let after = db.strategies().await.unwrap().into_iter().find(|s| s.id == sid).unwrap();
        assert_eq!(after.version, 2, "approval bumps the version");
        assert!(!after.rules.contains("\"maxNames\":1"), "rules were updated: {}", after.rules);

        // approving again is idempotent (already resolved).
        db.resolve_improvement(&prop_id, true).await.unwrap();
        let again = db.strategies().await.unwrap().into_iter().find(|s| s.id == sid).unwrap();
        assert_eq!(again.version, 2);
    }

    #[tokio::test]
    async fn reject_leaves_strategy_unchanged() {
        let db = bootstrap("sqlite::memory:").await.unwrap();
        let hh = &db.households().await.unwrap()[0].id;
        let sid = db.create_strategy("Hold", "rotation", "", None, "{}").await.unwrap();
        let aid = db
            .create_allocation(hh, "Reject Test", "", 3, 10_000.0, None, Some(&sid))
            .await
            .unwrap();
        // a grey-zone candidate → all holds → proposes a distress-sell rule.
        let zones = HashMap::from([("AAPL".to_string(), Zone::Grey)]);
        let quotes = [("AAPL".to_string(), 200.0)].into_iter().collect();
        db.run_loop_tick(&aid, &["AAPL".to_string()], &zones, &quotes).await.unwrap();
        let pid = db.propose_improvement(&aid).await.unwrap().unwrap();
        db.resolve_improvement(&pid, false).await.unwrap();
        let s = db.strategies().await.unwrap().into_iter().find(|s| s.id == sid).unwrap();
        assert_eq!(s.version, 1, "rejected proposal must not change the strategy");
    }
}
