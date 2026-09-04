//! Veyra Runtime: the OS layer that uses intelligence engines
//! (Plan.txt sections 22, 41-43).
//!
//! This crate currently provides the two foundational runtime pieces:
//!
//! - [`RuntimeState`] — the explicit task state machine
//!   (`IDLE → UNDERSTANDING → PLANNING → EXECUTING → VERIFYING → COMPLETED`,
//!   with `FAILED → REPLAN` recovery and `INTERRUPTED` from any state).
//! - [`EventBus`] — the decoupling mechanism between runtime components.

/// Errors raised on invalid state transitions.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid transition: {from:?} -> {to:?}")]
pub struct InvalidTransition {
    pub from: RuntimeState,
    pub to: RuntimeState,
}

/// Explicit states of a runtime task (Plan.txt section 42).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    Idle,
    Understanding,
    Planning,
    Executing,
    Verifying,
    Completed,
    Failed,
    Replan,
    Interrupted,
}

impl RuntimeState {
    /// Whether the transition `self -> next` is allowed.
    pub fn can_transition_to(&self, next: RuntimeState) -> bool {
        use RuntimeState::*;
        matches!(
            (self, next),
            (Idle, Understanding)
                | (Understanding, Planning)
                | (Understanding, Idle)
                | (Planning, Executing)
                | (Planning, Failed)
                | (Executing, Verifying)
                | (Executing, Failed)
                | (Verifying, Completed)
                | (Verifying, Failed)
                | (Failed, Replan)
                | (Replan, Executing)
                | (Completed, Idle)
                | (Interrupted, Understanding)
                | (Interrupted, Idle)
        ) || matches!(next, RuntimeState::Interrupted)
    }

    /// Validate and perform a transition, returning the new state.
    pub fn transition(&self, next: RuntimeState) -> Result<RuntimeState, InvalidTransition> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(InvalidTransition { from: *self, to: next })
        }
    }
}

/// A subscriber callback registered on the bus.
pub type SubscriberFn<T> = std::sync::Arc<dyn Fn(&T) + Send + Sync>;

/// A simple synchronous in-process event bus.
///
/// The Event Bus is not merely a messaging layer — it is the mechanism that
/// keeps Runtime components decoupled (Plan.txt section 41).
///
/// Note: `publish` iterates a snapshot of subscribers, so subscribing while
/// publishing is safe but the new subscriber only sees later events.
pub struct EventBus<T> {
    subscribers: Vec<SubscriberFn<T>>,
}

impl<T> Default for EventBus<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> EventBus<T> {
    pub fn new() -> Self {
        Self { subscribers: Vec::new() }
    }

    /// Register a subscriber that is called for every published event.
    pub fn subscribe(&mut self, f: impl Fn(&T) + Send + Sync + 'static) {
        self.subscribers.push(std::sync::Arc::new(f) as SubscriberFn<T>);
    }

    /// Number of registered subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }

    /// Publish an event to all subscribers, in subscription order.
    pub fn publish(&self, event: &T) {
        for sub in &self.subscribers {
            sub(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veyra_core::event::EventType;

    #[test]
    fn happy_path_state_machine() {
        let s = RuntimeState::Idle;
        let s = s.transition(RuntimeState::Understanding).unwrap();
        let s = s.transition(RuntimeState::Planning).unwrap();
        let s = s.transition(RuntimeState::Executing).unwrap();
        let s = s.transition(RuntimeState::Verifying).unwrap();
        let s = s.transition(RuntimeState::Completed).unwrap();
        let s = s.transition(RuntimeState::Idle).unwrap();
        assert_eq!(s, RuntimeState::Idle);
    }

    #[test]
    fn failure_recovery_loop() {
        let s = RuntimeState::Executing;
        let s = s.transition(RuntimeState::Failed).unwrap();
        let s = s.transition(RuntimeState::Replan).unwrap();
        let s = s.transition(RuntimeState::Executing).unwrap();
        assert_eq!(s, RuntimeState::Executing);
    }

    #[test]
    fn interrupt_is_possible_from_any_state() {
        for state in [
            RuntimeState::Understanding,
            RuntimeState::Planning,
            RuntimeState::Executing,
            RuntimeState::Verifying,
            RuntimeState::Failed,
        ] {
            assert!(state.can_transition_to(RuntimeState::Interrupted));
        }
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        assert!(RuntimeState::Idle.transition(RuntimeState::Completed).is_err());
        assert!(RuntimeState::Idle.transition(RuntimeState::Verifying).is_err());
        assert!(RuntimeState::Completed.transition(RuntimeState::Executing).is_err());
    }

    #[test]
    fn event_bus_delivers_to_all_subscribers_in_order() {
        let bus: std::sync::Arc<std::sync::Mutex<EventBus<EventType>>> =
            std::sync::Arc::new(std::sync::Mutex::new(EventBus::new()));

        let received: std::sync::Arc<std::sync::Mutex<Vec<EventType>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        {
            let mut bus = bus.lock().unwrap();
            let sink = received.clone();
            bus.subscribe(move |e| sink.lock().unwrap().push(*e));
            let sink2 = received.clone();
            bus.subscribe(move |e| sink2.lock().unwrap().push(*e));
        }

        bus.lock().unwrap().publish(&EventType::GoalCreated);
        bus.lock().unwrap().publish(&EventType::VerificationCompleted);

        let got = received.lock().unwrap().clone();
        // Each event is delivered to every subscriber (subscription order):
        // event 1 → sub1, sub2; event 2 → sub1, sub2.
        assert_eq!(
            got,
            vec![
                EventType::GoalCreated,
                EventType::GoalCreated,
                EventType::VerificationCompleted,
                EventType::VerificationCompleted,
            ]
        );
        assert_eq!(bus.lock().unwrap().subscriber_count(), 2);
    }
}
