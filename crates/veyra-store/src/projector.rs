//! Current State Projection (Plan.txt section 17).
//!
//! Current state is a materialized projection of historical events:
//!
//! ```text
//! Event Log → Projector → Goals / Project State / Decisions / Issues
//! ```
//!
//! This allows Veyra to reconstruct state after a crash or database
//! recovery. The projection is pure: it never mutates the event log.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use veyra_core::{Decision, Event, EventType, Goal, VerificationResult};

/// A chat turn projected from `user_input` / `assistant_response` events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    /// "user" or "assistant".
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// The materialized current state of a project, rebuilt from its events.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectSnapshot {
    pub goals: Vec<Goal>,
    pub decisions: Vec<Decision>,
    pub user_feedback: Vec<String>,
    pub last_verification: Option<VerificationResult>,
    pub messages: Vec<ChatMessage>,
}

impl ProjectSnapshot {
    /// Look up a goal by id.
    pub fn goal(&self, id: &str) -> Option<&Goal> {
        self.goals.iter().find(|g| g.id == id)
    }
}

/// Payload of `goal_created` / `goal_updated` events.
#[derive(Debug, Deserialize)]
struct GoalPayload {
    goal: Goal,
}

/// Payload of `user_feedback` events.
#[derive(Debug, Deserialize)]
struct FeedbackPayload {
    content: String,
}

/// Rebuilds [`ProjectSnapshot`] from an ordered slice of events.
pub struct Projector;

impl Projector {
    /// Fold an ordered event list into the current state.
    ///
    /// Payload conventions (documented here so producers and the projector
    /// stay in sync):
    ///
    /// - `goal_created` / `goal_updated`: `{ "goal": Goal }` — an update
    ///   replaces the goal with the same id (append-only correction).
    /// - `decision_recorded`: `Decision`
    /// - `user_feedback`: `{ "content": String }`
    /// - `verification_completed`: `VerificationResult`
    pub fn project(events: &[Event]) -> ProjectSnapshot {
        let mut snapshot = ProjectSnapshot::default();

        for event in events {
            match event.event_type {
                EventType::GoalCreated | EventType::GoalUpdated => {
                    if let Ok(payload) = serde_json::from_value::<GoalPayload>(event.payload.clone())
                    {
                        match snapshot.goals.iter_mut().find(|g| g.id == payload.goal.id) {
                            Some(existing) => *existing = payload.goal,
                            None => snapshot.goals.push(payload.goal),
                        }
                    }
                }
                EventType::DecisionRecorded => {
                    if let Ok(decision) = serde_json::from_value::<Decision>(event.payload.clone())
                    {
                        snapshot.decisions.push(decision);
                    }
                }
                EventType::UserFeedback => {
                    if let Ok(feedback) =
                        serde_json::from_value::<FeedbackPayload>(event.payload.clone())
                    {
                        snapshot.user_feedback.push(feedback.content);
                    }
                }
                EventType::UserInput => {
                    if let Ok(feedback) =
                        serde_json::from_value::<FeedbackPayload>(event.payload.clone())
                    {
                        snapshot.messages.push(ChatMessage {
                            role: "user".into(),
                            content: feedback.content,
                            created_at: event.created_at,
                        });
                    }
                }
                EventType::AssistantResponse => {
                    if let Ok(feedback) =
                        serde_json::from_value::<FeedbackPayload>(event.payload.clone())
                    {
                        snapshot.messages.push(ChatMessage {
                            role: "assistant".into(),
                            content: feedback.content,
                            created_at: event.created_at,
                        });
                    }
                }
                EventType::VerificationCompleted => {
                    if let Ok(result) =
                        serde_json::from_value::<VerificationResult>(event.payload.clone())
                    {
                        snapshot.last_verification = Some(result);
                    }
                }
                // Event types that do not contribute to the projection yet
                // (tool runs, raw inputs, reconciliation markers, ...).
                _ => {}
            }
        }

        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use veyra_core::{GoalStatus, RequirementStatus};

    fn sample_goal(id: &str, objective: &str) -> Goal {
        Goal {
            id: id.into(),
            project_id: "proj-1".into(),
            objective: objective.into(),
            requirements: vec![],
            constraints: vec![],
            preferences: vec![],
            acceptance_criteria: vec![],
            status: GoalStatus::Active,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn projection_rebuilds_state_from_events() {
        let decision = Decision {
            id: "dec-1".into(),
            project_id: "proj-1".into(),
            decision: "Use Tauri instead of Electron.".into(),
            rationale: Some("Lower memory usage and native Rust runtime.".into()),
            created_at: chrono::Utc::now(),
        };
        let verification = VerificationResult {
            id: "verif-1".into(),
            passed: true,
            summary: "Build: PASS, Tests: 24/24 PASS".into(),
            verified_at: chrono::Utc::now(),
        };

        let events = vec![
            Event::now("proj-1", EventType::GoalCreated, json!({ "goal": sample_goal("goal-1", "Build a Windows AI assistant.") })),
            Event::now("proj-1", EventType::DecisionRecorded, serde_json::to_value(&decision).unwrap()),
            Event::now("proj-1", EventType::UserFeedback, json!({"content": "ここ違う"})),
            Event::now("proj-1", EventType::UserFeedback, json!({"content": "もっとこうして"})),
            Event::now("proj-1", EventType::VerificationCompleted, serde_json::to_value(&verification).unwrap()),
        ];

        let snapshot = Projector::project(&events);
        assert_eq!(snapshot.goals.len(), 1);
        assert_eq!(snapshot.goals[0].objective, "Build a Windows AI assistant.");
        assert_eq!(snapshot.decisions.len(), 1);
        assert_eq!(snapshot.decisions[0].decision, "Use Tauri instead of Electron.");
        assert_eq!(snapshot.user_feedback, vec!["ここ違う", "もっとこうして"]);
        assert!(snapshot.last_verification.as_ref().unwrap().passed);
    }

    #[test]
    fn goal_update_replaces_without_rewriting_history() {
        let mut goal_v2 = sample_goal("goal-1", "Dark minimal interface (updated)");
        goal_v2.status = GoalStatus::Paused;
        goal_v2.acceptance_criteria.push(veyra_core::AcceptanceCriterion {
            id: "AC-001".into(),
            goal_id: "goal-1".into(),
            content: "Application launches without error.".into(),
            status: RequirementStatus::Planned,
        });

        let events = vec![
            Event::now("proj-1", EventType::GoalCreated, json!({ "goal": sample_goal("goal-1", "Dark minimal interface") })),
            Event::now("proj-1", EventType::GoalUpdated, json!({ "goal": goal_v2 })),
        ];

        // History is preserved: both events exist, in order.
        assert_eq!(events[0].event_type, EventType::GoalCreated);
        assert_eq!(events[1].event_type, EventType::GoalUpdated);

        // The projection reflects the *latest* goal state, without duplicates.
        let snapshot = Projector::project(&events);
        assert_eq!(snapshot.goals.len(), 1);
        assert_eq!(snapshot.goal("goal-1").unwrap().objective, "Dark minimal interface (updated)");
        assert_eq!(snapshot.goal("goal-1").unwrap().status, GoalStatus::Paused);
        assert_eq!(snapshot.goal("goal-1").unwrap().acceptance_criteria.len(), 1);
    }
}


