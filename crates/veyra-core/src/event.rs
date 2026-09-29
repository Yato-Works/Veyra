//! The append-only Event Log — the historical source of truth
//! (Plan.txt sections 15-16).
//!
//! Event log entries are append-only. Historical events are never edited;
//! corrections are expressed by appending a corrective event.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ProjectId;

/// Types of events produced by meaningful state transitions (Plan.txt §15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    UserInput,
    IntentDetected,
    GoalCreated,
    GoalUpdated,
    RequirementAdded,
    DecisionRecorded,
    ToolRequested,
    ToolStarted,
    ToolCompleted,
    ToolFailed,
    FileModified,
    ExternalFileModification,
    VerificationStarted,
    VerificationCompleted,
    UserFeedback,
    AssistantResponse,
    StateReconciled,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::UserInput => "user_input",
            EventType::IntentDetected => "intent_detected",
            EventType::GoalCreated => "goal_created",
            EventType::GoalUpdated => "goal_updated",
            EventType::RequirementAdded => "requirement_added",
            EventType::DecisionRecorded => "decision_recorded",
            EventType::ToolRequested => "tool_requested",
            EventType::ToolStarted => "tool_started",
            EventType::ToolCompleted => "tool_completed",
            EventType::ToolFailed => "tool_failed",
            EventType::FileModified => "file_modified",
            EventType::ExternalFileModification => "external_file_modification",
            EventType::VerificationStarted => "verification_started",
            EventType::VerificationCompleted => "verification_completed",
            EventType::UserFeedback => "user_feedback",
            EventType::AssistantResponse => "assistant_response",
            EventType::StateReconciled => "state_reconciled",
        }
    }

    /// Parse the canonical event type string. Unknown strings are reported
    /// as `None` so callers can decide how to handle future event types.
    pub fn parse(s: &str) -> Option<Self> {
        let t = match s {
            "user_input" => EventType::UserInput,
            "intent_detected" => EventType::IntentDetected,
            "goal_created" => EventType::GoalCreated,
            "goal_updated" => EventType::GoalUpdated,
            "requirement_added" => EventType::RequirementAdded,
            "decision_recorded" => EventType::DecisionRecorded,
            "tool_requested" => EventType::ToolRequested,
            "tool_started" => EventType::ToolStarted,
            "tool_completed" => EventType::ToolCompleted,
            "tool_failed" => EventType::ToolFailed,
            "file_modified" => EventType::FileModified,
            "external_file_modification" => EventType::ExternalFileModification,
            "verification_started" => EventType::VerificationStarted,
            "verification_completed" => EventType::VerificationCompleted,
            "user_feedback" => EventType::UserFeedback,
            "assistant_response" => EventType::AssistantResponse,
            "state_reconciled" => EventType::StateReconciled,
            _ => return None,
        };
        Some(t)
    }
}

/// A single entry of the append-only event log (Plan.txt section 15).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub project_id: ProjectId,
    pub session_id: Option<String>,

    pub event_type: EventType,
    pub payload: Value,

    pub created_at: DateTime<Utc>,
}

impl Event {
    /// Convenience constructor that assigns a fresh id and timestamp.
    pub fn now(project_id: impl Into<String>, event_type: EventType, payload: Value) -> Self {
        Self {
            id: uuid_v4(),
            project_id: project_id.into(),
            session_id: None,
            event_type,
            payload,
            created_at: Utc::now(),
        }
    }
}

/// Generate a unique UUIDv4 string.
fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_type_strings_round_trip() {
        for t in [
            EventType::UserInput,
            EventType::IntentDetected,
            EventType::GoalCreated,
            EventType::GoalUpdated,
            EventType::RequirementAdded,
            EventType::DecisionRecorded,
            EventType::ToolRequested,
            EventType::ToolStarted,
            EventType::ToolCompleted,
            EventType::ToolFailed,
            EventType::FileModified,
            EventType::ExternalFileModification,
            EventType::VerificationStarted,
            EventType::VerificationCompleted,
            EventType::UserFeedback,
            EventType::AssistantResponse,
            EventType::StateReconciled,
        ] {
            assert_eq!(EventType::parse(t.as_str()), Some(t));
            let json = serde_json::to_value(t).unwrap();
            let back: EventType = serde_json::from_value(json).unwrap();
            assert_eq!(back, t);
        }
        assert_eq!(EventType::parse("something_new"), None);
    }
}
