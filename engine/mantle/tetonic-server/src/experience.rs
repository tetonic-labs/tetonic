//! Agent-owned, run-scoped experience. Statements never become observed world facts.
use serde_json::{json,Value};
use std::collections::VecDeque;

#[derive(Default)]
pub struct ExperienceMemory {
    scope:Value,
    evidence:VecDeque<Value>,
    seen:VecDeque<String>,
    intention:Value,
    revision:Value,
}
impl ExperienceMemory {
    pub fn scope(&mut self, data:&Value) {
        let scope=json!([data["memory_scope"],data["agent_id"]]);
        if self.scope!=scope { *self=Self {scope,..Default::default()}; }
        if self.revision!=data["_world_context_revision"] {self.intention=Value::Null;self.revision=data["_world_context_revision"].clone();}
    }
    pub fn has_new_events(&self, events:&[tetonic_domain::WorldEvent])->bool {
        events.iter().any(|e|json!(e)["payload"]["_delivery"]["id"].as_str().is_some_and(|id|!self.seen.iter().any(|known|known==id)))
    }
    pub fn retain(&mut self, events:&[tetonic_domain::WorldEvent]) {
        for event in events {
            let value=json!(event);
            let Some(id)=value["payload"]["_delivery"]["id"].as_str() else {continue;};
            if self.seen.iter().any(|s|s==id){continue;}
            let category=match event.kind.as_str(){"agent_spoke"=>"heard_statement","own_utterance"=>"own_utterance","private_suggestion"=>"external_suggestion","action_outcome"|"task_outcome"=>"world_outcome",_=>"world_event"};
            self.seen.push_back(id.to_owned());
            self.evidence.push_back(json!({"category":category,"source_event":value}));
            if self.seen.len()>256 {self.seen.pop_front();}
            if self.evidence.len()>64 {self.evidence.pop_front();}
        }
    }
    pub fn recalled(&self, current_ids:&[String])->Vec<Value> {
        // Prefer actual outcomes/incoming information over repeatedly recalling our own speech.
        let mut selected:Vec<_>=self.evidence.iter().rev().filter(|e|e["category"]!="own_utterance" && !current_ids.iter().any(|id|e["source_event"]["payload"]["_delivery"]["id"]==*id)).take(4).cloned().collect();
        selected.reverse();selected
    }
    pub fn intention(&self)->Value { self.intention.clone() }
    pub fn update_intention(&mut self, decision:&Value) {if let Some(value)=decision.get("intention"){self.intention=value.clone();}}
}

pub fn validate_intention(decision:&Value)->Result<(),String> {
    if let Some(v)=decision.get("intention") {
        if v.is_null(){return Ok(());}
        let Some(o)=v.as_object() else{return Err("intention must be null or an object".into());};
        if o.keys().any(|k|!matches!(k.as_str(),"purpose"|"next_step"|"revision_reason")) {return Err("unsupported intention field".into());}
        if !o.get("purpose").is_some_and(|v|v.as_str().is_some_and(|s|!s.trim().is_empty())) {return Err("intention requires purpose".into());}
        if o.values().any(|v|!v.as_str().is_some_and(|s|s.chars().count()<=240)){return Err("intention fields must be strings of at most 240 characters".into());}
    }
    Ok(())
}

#[cfg(test)]mod tests{
 use super::*;
 fn event(id:&str,kind:&str)->tetonic_domain::WorldEvent{serde_json::from_value(json!({"kind":kind,"source":"world","urgency":"low","payload":{"text":"The well is empty","_delivery":{"id":id}}})).unwrap()}
 #[test]fn isolates_provenance_deduplicates_and_preserves_experience_across_objectives(){
  let mut m=ExperienceMemory::default();let mut data=json!({"memory_scope":"world","agent_id":"a","_world_context_revision":0});m.scope(&data);
  m.retain(&[event("1","agent_spoke"),event("1","agent_spoke"),event("2","own_utterance")]);
  assert_eq!(m.evidence.len(),2);assert_eq!(m.recalled(&[])[0]["category"],"heard_statement");
  m.update_intention(&json!({"intention":{"purpose":"Learn about the area"}}));data["_world_context_revision"]=json!(1);m.scope(&data);
  assert!(m.intention().is_null());assert_eq!(m.recalled(&[]).len(),1);
  data["agent_id"]=json!("b");m.scope(&data);assert!(m.recalled(&[]).is_empty());
 }
 #[test]fn intention_is_optional_bounded_and_explicitly_clearable(){
  assert!(validate_intention(&json!({})).is_ok());assert!(validate_intention(&json!({"intention":null})).is_ok());
  assert!(validate_intention(&json!({"intention":{"purpose":"Observe", "next_step":"Wait"}})).is_ok());
  assert!(validate_intention(&json!({"intention":{"purpose":"x".repeat(241)}})).is_err());
  assert!(validate_intention(&json!({"intention":{"mood":"happy"}})).is_err());
 }
}
