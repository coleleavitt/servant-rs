//! Communities / TAMP (Phase 6 parity). A strategist-model marketplace: SubAdvisors publish
//! Models, advisors browse + "subscribe" (PAPER subscriptions — no money moves, PHILOSOPHY.md).
//! Pure marketplace logic (search/compare/eligibility); persistence is a thin DB table. The
//! recon (services/communities.orion.com-ts) mapped Orion's real surface — this is the honest,
//! analysis-only slice.

use serde::{Deserialize, Serialize};

use crate::domain::ModelHolding;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrategistModel {
    pub id: String,
    pub sub_advisor: String,
    pub name: String,
    pub category: String, // "Equity" | "Fixed Income" | "Balanced" | "ESG" | ...
    pub min_investment: f64,
    pub holdings: Vec<ModelHolding>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelComparison {
    pub a: String,
    pub b: String,
    pub overlap_pct: f64, // % weight shared by common tickers
    pub a_unique: Vec<String>,
    pub b_unique: Vec<String>,
}

/// Whether an account with `aum` can subscribe to a model (meets the minimum investment).
pub fn eligible(model: &StrategistModel, aum: f64) -> bool {
    aum >= model.min_investment
}

/// Filter marketplace models by a free-text query over name/sub-advisor/category, case-insensitive.
pub fn search<'a>(models: &'a [StrategistModel], query: &str) -> Vec<&'a StrategistModel> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return models.iter().collect();
    }
    models
        .iter()
        .filter(|m| {
            m.name.to_lowercase().contains(&q)
                || m.sub_advisor.to_lowercase().contains(&q)
                || m.category.to_lowercase().contains(&q)
        })
        .collect()
}

/// Compare two strategist models: overlap (shared weight) + each model's unique tickers.
pub fn compare(a: &StrategistModel, b: &StrategistModel) -> ModelComparison {
    use std::collections::BTreeMap;
    let aw: BTreeMap<String, f64> = a
        .holdings
        .iter()
        .map(|h| (h.ticker.to_uppercase(), h.target_weight))
        .collect();
    let bw: BTreeMap<String, f64> = b
        .holdings
        .iter()
        .map(|h| (h.ticker.to_uppercase(), h.target_weight))
        .collect();
    // overlap = sum of min(weight) over common tickers
    let overlap: f64 = aw
        .iter()
        .filter_map(|(t, wa)| bw.get(t).map(|wb| wa.min(*wb)))
        .sum();
    let a_unique: Vec<String> = aw
        .keys()
        .filter(|t| !bw.contains_key(*t))
        .cloned()
        .collect();
    let b_unique: Vec<String> = bw
        .keys()
        .filter(|t| !aw.contains_key(*t))
        .cloned()
        .collect();
    ModelComparison {
        a: a.name.clone(),
        b: b.name.clone(),
        overlap_pct: overlap,
        a_unique,
        b_unique,
    }
}

/// A small seed marketplace so the page has content (replaceable from the DB).
pub fn seed_models() -> Vec<StrategistModel> {
    vec![
        StrategistModel {
            id: "m-core-eq".into(),
            sub_advisor: "Ponoma Strategies".into(),
            name: "Core Equity".into(),
            category: "Equity".into(),
            min_investment: 25_000.0,
            holdings: vec![
                ModelHolding {
                    ticker: "VTI".into(),
                    target_weight: 70.0,
                },
                ModelHolding {
                    ticker: "VXUS".into(),
                    target_weight: 30.0,
                },
            ],
        },
        StrategistModel {
            id: "m-balanced".into(),
            sub_advisor: "Ponoma Strategies".into(),
            name: "Balanced 60/40".into(),
            category: "Balanced".into(),
            min_investment: 10_000.0,
            holdings: vec![
                ModelHolding {
                    ticker: "VTI".into(),
                    target_weight: 60.0,
                },
                ModelHolding {
                    ticker: "BND".into(),
                    target_weight: 40.0,
                },
            ],
        },
        StrategistModel {
            id: "m-esg".into(),
            sub_advisor: "Green Capital".into(),
            name: "ESG Leaders".into(),
            category: "ESG".into(),
            min_investment: 50_000.0,
            holdings: vec![
                ModelHolding {
                    ticker: "ESGU".into(),
                    target_weight: 80.0,
                },
                ModelHolding {
                    ticker: "BND".into(),
                    target_weight: 20.0,
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eligibility_checks_minimum() {
        let m = &seed_models()[2]; // ESG, min 50k
        assert!(eligible(m, 60_000.0));
        assert!(!eligible(m, 40_000.0));
    }

    #[test]
    fn search_matches_category_and_advisor() {
        let models = seed_models();
        assert_eq!(search(&models, "esg").len(), 1);
        assert_eq!(search(&models, "ponoma").len(), 2);
        assert_eq!(search(&models, "").len(), 3);
    }

    #[test]
    fn compare_finds_overlap_and_unique() {
        let models = seed_models();
        let c = compare(&models[0], &models[1]); // Core Equity vs Balanced — share VTI
        // Core VTI 70, Balanced VTI 60 → overlap min = 60
        assert_eq!(c.overlap_pct, 60.0);
        assert!(c.a_unique.contains(&"VXUS".to_string()));
        assert!(c.b_unique.contains(&"BND".to_string()));
    }
}
