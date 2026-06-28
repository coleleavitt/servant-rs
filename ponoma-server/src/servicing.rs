//! Servicing / BackOffice (Phase 6, Connect parity). Service requests with a typed lifecycle
//! (Open → InProgress → Resolved / Cancelled) + a lightweight job-runner descriptor (the
//! QuartzJobRunner analogue: named recurring jobs with a cadence). Pure lifecycle logic here;
//! persistence is a thin DB table. No money/PII — operational workflow only.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestStatus {
    Open,
    InProgress,
    Resolved,
    Cancelled,
}

impl RequestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RequestStatus::Open => "Open",
            RequestStatus::InProgress => "InProgress",
            RequestStatus::Resolved => "Resolved",
            RequestStatus::Cancelled => "Cancelled",
        }
    }
    pub fn from_str_name(s: &str) -> Self {
        match s {
            "InProgress" => RequestStatus::InProgress,
            "Resolved" => RequestStatus::Resolved,
            "Cancelled" => RequestStatus::Cancelled,
            _ => RequestStatus::Open,
        }
    }
    pub fn is_terminal(self) -> bool {
        matches!(self, RequestStatus::Resolved | RequestStatus::Cancelled)
    }
}

/// Service-request topics (mirrors Eclipse BackOffice/Servicing topics).
pub const TOPICS: &[&str] = &[
    "Address Change",
    "Beneficiary Update",
    "Distribution Request",
    "Contribution",
    "Account Transfer (ACAT)",
    "Tax Document",
    "Money Movement",
    "Other",
];

/// Can a service request move `from` → `to`? Terminal states are final; Open/InProgress can
/// advance or cancel. This is the workflow guard the DB/UI consult.
pub fn can_transition(from: RequestStatus, to: RequestStatus) -> bool {
    use RequestStatus::*;
    if from.is_terminal() {
        return false;
    }
    matches!(
        (from, to),
        (Open, InProgress) | (Open, Cancelled) | (InProgress, Resolved) | (InProgress, Cancelled)
    )
}

/// A recurring back-office job (QuartzJobRunner analogue): name + cadence + whether enabled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub name: String,
    pub cadence: String, // "daily" | "weekly" | "monthly" | a cron-ish hint
    pub enabled: bool,
    pub last_run: Option<String>,
}

/// The default ponoma job set — the operational cadence a real desk would run.
pub fn default_jobs() -> Vec<ScheduledJob> {
    vec![
        ScheduledJob {
            name: "Refresh quotes".into(),
            cadence: "daily".into(),
            enabled: true,
            last_run: None,
        },
        ScheduledJob {
            name: "Recompute distress scores".into(),
            cadence: "weekly".into(),
            enabled: true,
            last_run: None,
        },
        ScheduledJob {
            name: "Drift / tolerance sweep".into(),
            cadence: "daily".into(),
            enabled: true,
            last_run: None,
        },
        ScheduledJob {
            name: "TLH scan".into(),
            cadence: "weekly".into(),
            enabled: true,
            last_run: None,
        },
        ScheduledJob {
            name: "Billing accrual".into(),
            cadence: "monthly".into(),
            enabled: true,
            last_run: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::RequestStatus::*;
    use super::*;

    #[test]
    fn open_can_start_or_cancel() {
        assert!(can_transition(Open, InProgress));
        assert!(can_transition(Open, Cancelled));
        assert!(!can_transition(Open, Resolved)); // must go through InProgress
    }

    #[test]
    fn in_progress_resolves_or_cancels() {
        assert!(can_transition(InProgress, Resolved));
        assert!(can_transition(InProgress, Cancelled));
        assert!(!can_transition(InProgress, Open)); // no going back
    }

    #[test]
    fn terminal_states_are_final() {
        assert!(!can_transition(Resolved, InProgress));
        assert!(!can_transition(Cancelled, Open));
        assert!(Resolved.is_terminal());
        assert!(!Open.is_terminal());
    }

    #[test]
    fn status_names_roundtrip() {
        for s in [Open, InProgress, Resolved, Cancelled] {
            assert_eq!(RequestStatus::from_str_name(s.as_str()), s);
        }
    }

    #[test]
    fn default_jobs_present() {
        let jobs = default_jobs();
        assert!(jobs.iter().any(|j| j.name == "TLH scan"));
        assert!(jobs.iter().all(|j| j.enabled));
    }
}
