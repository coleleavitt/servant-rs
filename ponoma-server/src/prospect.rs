//! AI Copilot — prospecting (Phase 7-8, AIProspects parity). Given a prospect's profile
//! (investable amount, risk tolerance, optional category preference), rank the strategist-model
//! marketplace by FIT: eligibility (meets minimum), risk match, and category preference. The
//! copilot narrates around this; the ranking math is pure + deterministic + auditable.

use serde::{Deserialize, Serialize};

use crate::communities::StrategistModel;

/// A prospect's profile. `risk` is 1 (conservative) .. 5 (aggressive).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProspectProfile {
    pub investable: f64,
    pub risk: u8,
    #[serde(default)]
    pub category_pref: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelFit {
    pub model_id: String,
    pub model_name: String,
    pub category: String,
    pub eligible: bool,
    pub fit_score: f64, // 0..1, higher = better fit
    pub reason: String,
}

/// Map a model's category to an implied risk level (1..5) for matching.
fn category_risk(category: &str) -> u8 {
    match category.to_lowercase().as_str() {
        "fixed income" | "bond" => 1,
        "balanced" => 3,
        "esg" => 3,
        "equity" => 4,
        "aggressive" | "growth" => 5,
        _ => 3,
    }
}

/// Rank the marketplace for a prospect. Fit = risk-match (closer implied risk → higher) +
/// category-preference bonus, gated by eligibility. Ineligible models score low but are still
/// returned (so the copilot can explain "needs $X more"). Sorted best-fit first.
pub fn rank_models(profile: &ProspectProfile, models: &[StrategistModel]) -> Vec<ModelFit> {
    let risk = profile.risk.clamp(1, 5);
    let mut out: Vec<ModelFit> = models
        .iter()
        .map(|m| {
            let eligible = profile.investable >= m.min_investment;
            let implied = category_risk(&m.category);
            // risk match: 1.0 when equal, decaying by 0.25 per level apart.
            let risk_match = (1.0 - (implied as f64 - risk as f64).abs() * 0.25).max(0.0);
            let cat_bonus = profile
                .category_pref
                .as_deref()
                .map(|p| {
                    if p.eq_ignore_ascii_case(&m.category) {
                        0.3
                    } else {
                        0.0
                    }
                })
                .unwrap_or(0.0);
            // risk_match (0..1) is weighted 0.7 so a category-preference bonus always separates
            // two equal-risk models without saturating at 1.0.
            let base = risk_match * 0.7 + cat_bonus;
            let fit_score = if eligible { base } else { base * 0.2 };
            let reason = if !eligible {
                format!("below minimum (needs ${:.0})", m.min_investment)
            } else if cat_bonus > 0.0 {
                format!("matches preferred category + risk level {risk}")
            } else {
                format!("risk fit {:.0}% for risk level {risk}", risk_match * 100.0)
            };
            ModelFit {
                model_id: m.id.clone(),
                model_name: m.name.clone(),
                category: m.category.clone(),
                eligible,
                fit_score,
                reason,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.fit_score
            .partial_cmp(&a.fit_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::communities::seed_models;

    #[test]
    fn conservative_prospect_prefers_balanced_over_equity() {
        // risk 2, $100k → eligible for all. Balanced (implied 3) closer to 2 than ESG/equity.
        let profile = ProspectProfile {
            investable: 100_000.0,
            risk: 2,
            category_pref: None,
        };
        let ranked = rank_models(&profile, &seed_models());
        assert!(ranked[0].eligible);
        // top fit should be the lowest-risk-distance model
        assert_eq!(ranked[0].model_name, "Balanced 60/40");
    }

    #[test]
    fn category_preference_boosts_fit() {
        let profile = ProspectProfile {
            investable: 100_000.0,
            risk: 3,
            category_pref: Some("ESG".into()),
        };
        let ranked = rank_models(&profile, &seed_models());
        assert_eq!(ranked[0].category, "ESG");
    }

    #[test]
    fn ineligible_models_rank_low_with_reason() {
        // $30k can't afford ESG (min 50k)
        let profile = ProspectProfile {
            investable: 30_000.0,
            risk: 3,
            category_pref: None,
        };
        let ranked = rank_models(&profile, &seed_models());
        let esg = ranked.iter().find(|m| m.category == "ESG").unwrap();
        assert!(!esg.eligible);
        assert!(esg.reason.contains("minimum"));
    }
}
