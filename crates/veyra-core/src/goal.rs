//! Goal is the most important persistent object in Veyra (Plan.txt section 10).
//!
//! Goal answers "what must be achieved". It survives context compression,
//! session ends and app restarts. It is durable but *not* immutable: every
//! change is expressed as an explicit `goal_updated` event in the event log.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Lifecycle of a Goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    Completed,
    Abandoned,
}

/// Lifecycle of a Requirement / Acceptance Criterion (Plan.txt section 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementStatus {
    Unknown,
    Planned,
    InProgress,
    Verified,
    Failed,
    Waived,
}

/// An explicit statement of what must exist (Plan.txt section 11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub goal_id: String,
    pub content: String,
    pub status: RequirementStatus,
}

/// Defines when Veyra may say "Done." (Plan.txt section 12).
///
/// Without satisfied acceptance criteria: `Goal.status != completed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub goal_id: String,
    pub content: String,
    pub status: RequirementStatus,
}

/// A restriction the solution must respect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraint {
    pub content: String,
}

/// A soft preference that guides choices but can be traded off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preference {
    pub content: String,
}

/// The persistent objective of a project (Plan.txt section 10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub project_id: String,

    pub objective: String,

    pub requirements: Vec<Requirement>,
    pub constraints: Vec<Constraint>,
    pub preferences: Vec<Preference>,

    pub acceptance_criteria: Vec<AcceptanceCriterion>,

    pub status: GoalStatus,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An important decision that must survive context compression
/// (Plan.txt section 14). A Decision is not just chat history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub project_id: String,
    pub decision: String,
    pub rationale: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_serialize_as_snake_case() {
        assert_eq!(
            serde_json::to_value(RequirementStatus::InProgress).unwrap(),
            serde_json::json!("in_progress")
        );
        assert_eq!(
            serde_json::to_value(GoalStatus::Active).unwrap(),
            serde_json::json!("active")
        );
    }
}
