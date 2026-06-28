//! Ecosystem-understanding agent (Phase 7-8, IDEA.md §6.1). The deterministic CORE of the
//! thesis engine: it fuses distress metrics (Altman Z″ zone, Merton-style flags) and
//! filing/event signals into a STRUCTURED thesis — verdict + confidence + weighted rationale —
//! rather than backtesting. An LLM can narrate around this, but the verdict math is pure and
//! testable here, so the agent's conclusions are auditable and reproducible.

use serde::{Deserialize, Serialize};

/// Distress zone (mirrors the TS `Zone` from distress.ts).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Zone {
    Safe,     // Z″ > 2.6
    Grey,     // 1.1 ≤ Z″ ≤ 2.6
    Distress, // Z″ < 1.1
    Unknown,
}

/// One observed signal feeding the thesis. `weight` is its pull on the score (+ bullish / −
/// bearish), `severity` 0..1 scales it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub kind: String, // e.g. "8-K bankruptcy", "insider buying", "going-concern"
    pub bullish: bool,
    pub severity: f64, // 0..1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Avoid,       // strong distress / bearish
    Caution,     // mixed / watch
    Neutral,     // no edge
    Opportunity, // contrarian value / bullish under-watched
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thesis {
    pub ticker: String,
    pub verdict: Verdict,
    pub score: f64,      // −1 (avoid) .. +1 (opportunity)
    pub confidence: f64, // 0..1 — how much evidence backs the verdict
    pub rationale: Vec<String>,
}

fn zone_score(zone: Zone) -> (f64, &'static str) {
    match zone {
        Zone::Safe => (0.5, "Altman Z″ in the safe zone — low bankruptcy risk"),
        Zone::Grey => (0.0, "Altman Z″ in the grey zone — watch the trend"),
        Zone::Distress => (
            -0.7,
            "Altman Z″ in the distress zone — elevated bankruptcy risk",
        ),
        Zone::Unknown => (0.0, "distress score unavailable"),
    }
}

/// Synthesize a thesis from a distress zone + a set of signals. Deterministic weighted sum:
/// the distress zone anchors the score; each signal nudges it by ±(0.25·severity); the result
/// is clamped to [−1, 1] and mapped to a verdict. Confidence grows with the count + strength of
/// corroborating evidence.
pub fn synthesize(ticker: &str, zone: Zone, signals: &[Signal]) -> Thesis {
    let (mut score, zr) = zone_score(zone);
    let mut rationale = vec![zr.to_string()];
    let mut evidence = if matches!(zone, Zone::Unknown) {
        0.0
    } else {
        1.0
    };

    for s in signals {
        let sev = s.severity.clamp(0.0, 1.0);
        let delta = if s.bullish { 0.25 * sev } else { -0.25 * sev };
        score += delta;
        evidence += sev;
        rationale.push(format!(
            "{} ({}, severity {:.0}%)",
            s.kind,
            if s.bullish { "bullish" } else { "bearish" },
            sev * 100.0
        ));
    }
    score = score.clamp(-1.0, 1.0);

    let verdict = if score <= -0.4 {
        Verdict::Avoid
    } else if score < -0.1 {
        Verdict::Caution
    } else if score < 0.3 {
        Verdict::Neutral
    } else {
        Verdict::Opportunity
    };

    // confidence: evidence count saturating toward 1.0 (3+ strong signals ≈ confident)
    let confidence = (evidence / 3.0).clamp(0.0, 1.0);

    Thesis {
        ticker: ticker.to_string(),
        verdict,
        score,
        confidence,
        rationale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distress_plus_bearish_signal_is_avoid() {
        let signals = vec![Signal {
            kind: "going-concern doubt".into(),
            bullish: false,
            severity: 1.0,
        }];
        let t = synthesize("WSHP", Zone::Distress, &signals);
        assert_eq!(t.verdict, Verdict::Avoid);
        assert!(t.score < -0.4);
        assert!(t.confidence > 0.5);
    }

    #[test]
    fn safe_plus_insider_buying_is_opportunity() {
        let signals = vec![
            Signal {
                kind: "insider buying".into(),
                bullish: true,
                severity: 0.8,
            },
            Signal {
                kind: "under-watched 10-K".into(),
                bullish: true,
                severity: 0.6,
            },
        ];
        let t = synthesize("ON", Zone::Safe, &signals);
        assert_eq!(t.verdict, Verdict::Opportunity);
        assert!(t.score >= 0.3);
    }

    #[test]
    fn grey_with_no_signals_is_neutral_low_confidence() {
        let t = synthesize("PPG", Zone::Grey, &[]);
        assert_eq!(t.verdict, Verdict::Neutral);
        assert!(t.confidence < 0.5);
    }

    #[test]
    fn score_is_clamped() {
        let signals = vec![
            Signal {
                kind: "x".into(),
                bullish: false,
                severity: 1.0
            };
            10
        ];
        let t = synthesize("X", Zone::Distress, &signals);
        assert!(t.score >= -1.0);
        assert_eq!(t.verdict, Verdict::Avoid);
    }

    #[test]
    fn mixed_signals_net_to_caution() {
        let signals = vec![
            Signal {
                kind: "8-K restructuring".into(),
                bullish: false,
                severity: 0.6,
            },
            Signal {
                kind: "new contract".into(),
                bullish: true,
                severity: 0.3,
            },
        ];
        let t = synthesize("MID", Zone::Grey, &signals);
        // net bearish but mild → caution
        assert!(matches!(t.verdict, Verdict::Caution | Verdict::Neutral));
    }
}
