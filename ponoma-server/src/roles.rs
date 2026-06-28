//! Role / permission model (Phase 6, Connect parity). Mirrors Eclipse's `roleTypeId` hierarchy
//! (OrionAdmin / FirmAdmin / TeamAdmin / User / APIOnly). A pure capability matrix: `can(role,
//! capability)` answers whether a role may perform an action. Deterministic — the agent/UI ask
//! this, they never invent permissions.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    OrionAdmin, // full platform admin
    FirmAdmin,  // admin within a firm
    TeamAdmin,  // admin within a team
    User,       // advisor / standard user
    ApiOnly,    // service account — no interactive UI
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::OrionAdmin => "OrionAdmin",
            Role::FirmAdmin => "FirmAdmin",
            Role::TeamAdmin => "TeamAdmin",
            Role::User => "User",
            Role::ApiOnly => "ApiOnly",
        }
    }
    /// Eclipse roleTypeId order: higher = more privilege (APIOnly is special/interactive-blocked).
    fn rank(self) -> u8 {
        match self {
            Role::OrionAdmin => 4,
            Role::FirmAdmin => 3,
            Role::TeamAdmin => 2,
            Role::User => 1,
            Role::ApiOnly => 0,
        }
    }
}

/// Capabilities gated by role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    ViewBook,         // read households/accounts/holdings
    PaperTrade,       // execute paper trades
    EditModels,       // create/edit models + security sets
    ManageHouseholds, // create households/accounts
    ManageBilling,    // edit fee schedules
    AdminConfig,      // platform/firm configuration
    InteractiveLogin, // may use the UI at all
}

/// Can `role` perform `capability`? The matrix encodes the privilege ladder + APIOnly's
/// interactive block (it can read/trade via API but cannot log in or admin).
pub fn can(role: Role, capability: Capability) -> bool {
    use Capability::*;
    use Role::*;
    match capability {
        InteractiveLogin => role != ApiOnly,
        ViewBook => true, // every role (incl. ApiOnly) can read the book
        PaperTrade => true,
        EditModels => role.rank() >= TeamAdmin.rank() || role == User, // advisors + team admins+
        ManageHouseholds => role.rank() >= TeamAdmin.rank(),
        ManageBilling => role.rank() >= FirmAdmin.rank(),
        AdminConfig => role == OrionAdmin || role == FirmAdmin,
    }
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::ViewBook => "ViewBook",
            Capability::PaperTrade => "PaperTrade",
            Capability::EditModels => "EditModels",
            Capability::ManageHouseholds => "ManageHouseholds",
            Capability::ManageBilling => "ManageBilling",
            Capability::AdminConfig => "AdminConfig",
            Capability::InteractiveLogin => "InteractiveLogin",
        }
    }
}

impl Role {
    pub fn from_str_name(s: &str) -> Self {
        match s {
            "OrionAdmin" => Role::OrionAdmin,
            "FirmAdmin" => Role::FirmAdmin,
            "TeamAdmin" => Role::TeamAdmin,
            "ApiOnly" => Role::ApiOnly,
            _ => Role::User,
        }
    }
}

const ALL_CAPS: &[Capability] = &[
    Capability::ViewBook,
    Capability::PaperTrade,
    Capability::EditModels,
    Capability::ManageHouseholds,
    Capability::ManageBilling,
    Capability::AdminConfig,
    Capability::InteractiveLogin,
];

/// The full capability map for a role — `(capability name, allowed)` for every capability.
/// The UI consults this to gate admin features.
pub fn capabilities(role: Role) -> Vec<(&'static str, bool)> {
    ALL_CAPS
        .iter()
        .map(|&c| (c.as_str(), can(role, c)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Capability::*;
    use super::Role::*;
    use super::*;

    #[test]
    fn capabilities_map_complete() {
        let caps = capabilities(FirmAdmin);
        assert_eq!(caps.len(), 7);
        assert!(caps.iter().any(|(c, ok)| *c == "ManageBilling" && *ok));
        assert!(caps.iter().any(|(c, ok)| *c == "AdminConfig" && *ok));
        let user = capabilities(User);
        assert!(user.iter().any(|(c, ok)| *c == "ManageBilling" && !*ok));
    }

    #[test]
    fn api_only_cannot_login_but_can_read() {
        assert!(!can(ApiOnly, InteractiveLogin));
        assert!(can(ApiOnly, ViewBook));
        assert!(!can(ApiOnly, ManageHouseholds));
    }

    #[test]
    fn billing_needs_firm_admin() {
        assert!(can(OrionAdmin, ManageBilling));
        assert!(can(FirmAdmin, ManageBilling));
        assert!(!can(TeamAdmin, ManageBilling));
        assert!(!can(User, ManageBilling));
    }

    #[test]
    fn user_can_trade_and_edit_models_but_not_admin() {
        assert!(can(User, PaperTrade));
        assert!(can(User, EditModels));
        assert!(!can(User, AdminConfig));
        assert!(!can(User, ManageHouseholds));
    }

    #[test]
    fn manage_households_needs_team_admin() {
        assert!(can(TeamAdmin, ManageHouseholds));
        assert!(can(FirmAdmin, ManageHouseholds));
        assert!(!can(User, ManageHouseholds));
    }

    #[test]
    fn role_names_roundtrip() {
        assert_eq!(OrionAdmin.as_str(), "OrionAdmin");
        assert_eq!(ApiOnly.as_str(), "ApiOnly");
    }
}
