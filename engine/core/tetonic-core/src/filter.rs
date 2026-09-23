//! Sensory filtering for continuous agent execution (SAE-104).
//!
//! Prevents high-frequency, repetitive sensory ticks from exhausting
//! context memory or inducing unnecessary model evaluations.

use std::collections::HashMap;
use tetonic_domain::{Perception, Urgency};

/// Evaluation outcome from the sensory filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterDecision {
    /// State has not significantly changed; drop this tick from memory and cognition.
    Drop,
    /// Sensory delta, discrete event, or elevated urgency detected; pass to brain.
    Pass,
}

/// State-tracking sensory filter that identifies meaningful world deltas.
#[derive(Debug, Clone, Default)]
pub struct SensoryFilter {
    last_sequence: u64,
    last_signal_values: HashMap<String, String>,
}

impl SensoryFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluate an incoming perception against historical state.
    pub fn filter(&mut self, perception: &Perception) -> FilterDecision {
        // High or Critical urgency ticks bypass filtering immediately
        if perception.urgency >= Urgency::High {
            return FilterDecision::Pass;
        }

        // Discrete occurrences (messages, alerts, petitions) always pass
        if !perception.events.is_empty() {
            return FilterDecision::Pass;
        }

        // Check if any signals indicate change
        let mut any_signal_changed = false;
        for sig in &perception.signals {
            if sig.changed {
                any_signal_changed = true;
                break;
            }
            let val_str = format!("{:?}", sig.value);
            if let Some(prev) = self.last_signal_values.get(&sig.name) {
                if prev != &val_str {
                    any_signal_changed = true;
                    break;
                }
            } else {
                any_signal_changed = true;
                break;
            }
        }

        if any_signal_changed {
            for sig in &perception.signals {
                self.last_signal_values
                    .insert(sig.name.clone(), format!("{:?}", sig.value));
            }
            self.last_sequence = perception.sequence;
            FilterDecision::Pass
        } else {
            FilterDecision::Drop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tetonic_domain::{Signal, SignalValue, WorldEvent, WorldState};

    fn make_perception(seq: u64, urgency: Urgency, signals: Vec<Signal>, events: Vec<WorldEvent>) -> Perception {
        Perception {
            when: Utc::now(),
            sequence: seq,
            urgency,
            signals,
            events,
            state: WorldState {
                schema_id: "test".into(),
                data: serde_json::Value::Null,
            },
        }
    }

    #[test]
    fn filter_drops_identical_idle_ticks() {
        let mut filter = SensoryFilter::new();

        let sig = Signal {
            name: "grain".into(),
            value: SignalValue::Int(100),
            changed: false,
            trend: None,
            urgency: Urgency::Low,
        };

        // First perception passes because grain was never seen
        let p1 = make_perception(1, Urgency::Low, vec![sig.clone()], vec![]);
        assert_eq!(filter.filter(&p1), FilterDecision::Pass);

        // Subsequent identical perceptions are dropped
        for seq in 2..=1000 {
            let p = make_perception(seq, Urgency::Low, vec![sig.clone()], vec![]);
            assert_eq!(filter.filter(&p), FilterDecision::Drop);
        }
    }

    #[test]
    fn filter_passes_on_events_or_high_urgency() {
        let mut filter = SensoryFilter::new();

        let sig = Signal {
            name: "grain".into(),
            value: SignalValue::Int(100),
            changed: false,
            trend: None,
            urgency: Urgency::Low,
        };

        let p1 = make_perception(1, Urgency::Low, vec![sig.clone()], vec![]);
        assert_eq!(filter.filter(&p1), FilterDecision::Pass);

        // High urgency passes even without signal changes
        let p2 = make_perception(2, Urgency::High, vec![sig.clone()], vec![]);
        assert_eq!(filter.filter(&p2), FilterDecision::Pass);

        // Discrete event passes
        let p3 = make_perception(
            3,
            Urgency::Low,
            vec![sig.clone()],
            vec![WorldEvent {
                kind: "user_message".into(),
                source: Some("operator".into()),
                payload: serde_json::json!({ "text": "hello" }),
                urgency: Urgency::Low,
            }],
        );
        assert_eq!(filter.filter(&p3), FilterDecision::Pass);
    }
}
