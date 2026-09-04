//! Veyra core domain types.
//!
//! This crate holds the persistent, authoritative objects of the Veyra
//! Runtime (see Plan.txt sections 10-17):
//!
//! - [`goal::Goal`] — what to achieve (durable, distinct from conversation)
//! - [`state::ProjectState`] — how things currently are
//! - [`event::Event`] — the historical source of truth (append-only)
//! - [`input::InputEvent`] — normalized user/application input
//!
//! Conversation context is *not* modeled here on purpose: it is temporary
//! and must never be confused with a Goal.

pub mod event;
pub mod goal;
pub mod input;
pub mod state;

// Root re-exports so consumers can `use veyra_core::{Goal, Event, ...}`.
pub use event::{Event, EventType};
pub use goal::{
    AcceptanceCriterion, Constraint, Decision, Goal, GoalStatus, Preference, Requirement,
    RequirementStatus,
};
pub use input::{InputContent, InputEvent, InputSource};
pub use state::{FileRef, Issue, Phase, ProjectState, VerificationResult};

/// Identifier of a Veyra project.
pub type ProjectId = String;
/// Identifier of a Goal.
pub type GoalId = String;
/// Identifier of a session (temporary conversation scope).
pub type SessionId = String;
/// Identifier of an event in the append-only event log.
pub type EventId = String;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_types_round_trip_through_json() {
        let goal = goal::Goal {
            id: "goal-1".into(),
            project_id: "proj-1".into(),
            objective: "Build a Windows AI assistant.".into(),
            requirements: vec![goal::Requirement {
                id: "REQ-001".into(),
                goal_id: "goal-1".into(),
                content: "Application must run on Windows 11.".into(),
                status: goal::RequirementStatus::Planned,
            }],
            constraints: vec![],
            preferences: vec![],
            acceptance_criteria: vec![goal::AcceptanceCriterion {
                id: "AC-001".into(),
                goal_id: "goal-1".into(),
                content: "Application launches without error.".into(),
                status: goal::RequirementStatus::Unknown,
            }],
            status: goal::GoalStatus::Active,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let json = serde_json::to_value(&goal).expect("serialize goal");
        let parsed: goal::Goal = serde_json::from_value(json).expect("deserialize goal");
        assert_eq!(parsed.objective, goal.objective);
        assert_eq!(parsed.requirements.len(), 1);
        assert_eq!(parsed.requirements[0].status, goal::RequirementStatus::Planned);
    }
}
