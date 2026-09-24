use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Mutex, time::{SystemTime, UNIX_EPOCH}};

#[derive(Default)]
struct Buffer { events: VecDeque<Value>, next: u64, bytes: usize }
pub struct TraceStore { pub enabled: bool, buffer: Mutex<Buffer>, health: Mutex<Value> }
impl TraceStore {
    pub fn new(enabled: bool) -> Self { Self { enabled, buffer: Mutex::new(Buffer::default()), health: Mutex::new(json!({"state":"waiting","last_success":null})) } }
    pub fn record(&self, trace_id: &str, stage: &str, mut data: Value) {
        let state = match stage {
            "inference_request" => Some("inferring"),
            "inference_response" => Some("validating"),
            "parsed_decision" => Some("action_pending"),
            "action_submit" => Some("acting"),
            "action_result" if data["success"] == false => Some("action_rejected"),
            "action_result" | "no_action" => Some("waiting"),
            "decision_error" | "parse_error" | "inference_error" | "action_error" => Some("failed"),
            _ => None,
        };
        if let Some(state) = state {
            let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
            let mut health=self.health.lock().unwrap();
            let last_success=if stage=="action_result" && data["success"]==true {json!(now)} else {health["last_success"].clone()};
            let failures=health["failure_count"].as_u64().unwrap_or(0)+u64::from(state=="failed" && health["last_failure"]["trace_id"].as_str()!=Some(trace_id));
            let last_failure=if state=="failed" {json!({"timestamp":now,"trace_id":trace_id,"stage":stage,"classification":data["classification"]})} else {health["last_failure"].clone()};
            // Health is payload-free. Raw model/provider text belongs only in opted-in trace.
            *health=json!({"state":state,"updated_at":now,"trace_id":trace_id,"stage":stage,"classification":data["classification"],"last_success":last_success,"failure_count":failures,"last_failure":last_failure});
        }
        if !self.enabled { return; }
        let encoded = data.to_string();
        if encoded.len() > 262144 { data = json!({"truncated":true,"original_bytes":encoded.len(),"reason":"event exceeds 256 KiB"}); }
        let mut b = self.buffer.lock().unwrap(); b.next += 1;
        let event=json!({"id":b.next,"timestamp":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),"trace_id":trace_id,"stage":stage,"data":data});
        b.bytes += event.to_string().len(); b.events.push_back(event);
        while b.events.len()>2048 || b.bytes>8*1024*1024 { if let Some(old)=b.events.pop_front(){b.bytes-=old.to_string().len();} }
    }
    pub fn health(&self) -> Value { self.health.lock().unwrap().clone() }
    pub fn since(&self, after:u64) -> Value {
        let b=self.buffer.lock().unwrap();
        let oldest=b.events.front().and_then(|e|e["id"].as_u64()).unwrap_or(b.next+1);
        json!({"enabled":self.enabled,"cursor":b.next,"oldest":oldest,"gap":after>0&&after+1<oldest,"events":b.events.iter().filter(|e|e["id"].as_u64().unwrap()>after).collect::<Vec<_>>()})
    }
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn health_survives_disabled_trace_without_retaining_payloads(){
  let store=TraceStore::new(false);
  store.record("a","inference_request",json!({"secret":"private prompt"}));
  assert_eq!(store.health()["state"],"inferring");
  store.record("a","parse_error",json!({"error":"private output"}));
  assert_eq!(store.health()["state"],"failed");
  assert!(!store.health().to_string().contains("private"));
  store.record("b","action_result",json!({"success":true}));
  assert_eq!(store.health()["state"],"waiting");
  assert!(store.health()["last_success"].is_number());
  assert_eq!(store.since(0)["cursor"],0);
 }
 #[test] fn bounded_cursor_and_disabled_capture(){
  let off=TraceStore::new(false);off.record("x","delta",json!("secret"));assert_eq!(off.since(0)["cursor"],0);
  let store=TraceStore::new(true);for _ in 0..2050 {store.record("x","delta",json!({"text":"chunk"}));}
  assert_eq!(store.since(0)["events"].as_array().unwrap().len(),2048);
  assert_eq!(store.since(1)["gap"],true);assert_eq!(store.since(2049)["events"].as_array().unwrap().len(),1);
 }
}
