//! Bounded prompt assembly for the configured continuous brain.
//! Estimates are deliberately explicit: UTF-8 bytes / 3 plus chat overhead.
//! This is not a model tokenizer; reserve a configurable safety margin as well.
use serde_json::{json, Value};

#[derive(Clone, Copy)]
pub struct ContextBudget {
    pub context: usize,
    pub completion: u32,
    pub margin: usize,
}

pub fn estimate(text: &str) -> usize { text.len().div_ceil(3) }

impl ContextBudget {
    pub fn assemble(&self, system: &str, current: &Value, intents: &[Value], memories: &[Value]) -> Result<(String, Value), String> {
        let allowance = self.context.checked_sub(self.completion as usize)
            .and_then(|n| n.checked_sub(self.margin))
            .ok_or("context cannot accommodate completion reserve and safety margin")?;
        let mut kept_intents = intents.to_vec();
        let mut kept_memories = memories.to_vec();
        loop {
            let input = format!("PRIOR INTENTS (not proof of success): {}\nLAST-SEEN MEMORIES (may be outdated): {}\nCURRENT AUTHORITATIVE LOCAL OBSERVATION (takes precedence over history): {}", json!(kept_intents), json!(kept_memories), current);
            let estimated = estimate(system) + estimate(&input) + 64;
            if estimated <= allowance {
                return Ok((input, json!({"accounting":"utf8_bytes_div_3_plus_64_chat_overhead","estimated":true,"context_tokens":self.context,"estimated_input_tokens":estimated,"completion_reserve":self.completion,"safety_margin":self.margin,"omitted_intents":intents.len()-kept_intents.len(),"omitted_memories":memories.len()-kept_memories.len(),"current_observation_preserved":true})));
            }
            // Selection never mutates the underlying memory store or current evidence.
            if !kept_memories.is_empty() { kept_memories.remove(0); }
            else if !kept_intents.is_empty() { kept_intents.remove(0); }
            else { return Err(format!("essential request estimate {estimated} exceeds input allowance {allowance}; reduce adapter payload or increase configured context")); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn growing_history_stays_bounded_without_dropping_current_evidence() {
        let budget=ContextBudget {context:4096,completion:384,margin:512};
        let current=json!({"events":[{"id":"must-retain"}],"state":{"epoch":2,"estopped":true,"position":[1,2]}});
        let mut memories=vec![];
        for n in 0..100 {
            memories.push(json!({"id":n,"fact":"old detail ".repeat(100)}));
            let (input,report)=budget.assemble("system",&current,&[],&memories).unwrap();
            assert!(report["estimated_input_tokens"].as_u64().unwrap()+384+512<=4096);
            assert!(input.contains(&current.to_string()));
        }
        assert_eq!(memories.len(),100);
    }
    #[test]
    fn essential_overflow_is_an_error_not_a_truncated_observation() {
        let budget=ContextBudget {context:1024,completion:256,margin:256};
        assert!(budget.assemble("system",&json!({"event":"x".repeat(4096)}),&[],&[]).is_err());
        assert!(ContextBudget {context:100,completion:256,margin:256}.assemble("",&json!({}),&[],&[]).is_err());
    }
    #[test]
    fn unicode_and_boundary_accounting_are_explicit() {
        assert_eq!(estimate("水"),1);
        let value=json!({"now":1});
        let b=ContextBudget {context:4096,completion:256,margin:256};
        let (_,r)=b.assemble("s",&value,&[],&[]).unwrap();
        let exact=r["estimated_input_tokens"].as_u64().unwrap() as usize+512;
        assert!(ContextBudget {context:exact,..b}.assemble("s",&value,&[],&[]).is_ok());
        assert!(ContextBudget {context:exact-1,..b}.assemble("s",&value,&[],&[]).is_err());
    }
}
