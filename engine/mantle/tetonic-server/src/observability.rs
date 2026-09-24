use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Mutex, time::{SystemTime, UNIX_EPOCH}};

#[derive(Default)]
struct Buffer { events: VecDeque<Value>, next: u64, bytes: usize }
pub struct TraceStore { pub enabled: bool, buffer: Mutex<Buffer> }
impl TraceStore {
    pub fn new(enabled: bool) -> Self { Self { enabled, buffer: Mutex::new(Buffer::default()) } }
    pub fn record(&self, trace_id: &str, stage: &str, mut data: Value) {
        if !self.enabled { return; }
        let encoded = data.to_string();
        if encoded.len() > 262144 { data = json!({"truncated":true,"original_bytes":encoded.len(),"reason":"event exceeds 256 KiB"}); }
        let mut b = self.buffer.lock().unwrap(); b.next += 1;
        let event=json!({"id":b.next,"timestamp":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),"trace_id":trace_id,"stage":stage,"data":data});
        b.bytes += event.to_string().len(); b.events.push_back(event);
        while b.events.len()>2048 || b.bytes>8*1024*1024 { if let Some(old)=b.events.pop_front(){b.bytes-=old.to_string().len();} }
    }
    pub fn since(&self, after:u64) -> Value {
        let b=self.buffer.lock().unwrap();
        let oldest=b.events.front().and_then(|e|e["id"].as_u64()).unwrap_or(b.next+1);
        json!({"enabled":self.enabled,"cursor":b.next,"oldest":oldest,"gap":after>0&&after+1<oldest,"events":b.events.iter().filter(|e|e["id"].as_u64().unwrap()>after).collect::<Vec<_>>()})
    }
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn bounded_cursor_and_disabled_capture(){
  let off=TraceStore::new(false);off.record("x","delta",json!("secret"));assert_eq!(off.since(0)["cursor"],0);
  let store=TraceStore::new(true);for _ in 0..2050 {store.record("x","delta",json!({"text":"chunk"}));}
  assert_eq!(store.since(0)["events"].as_array().unwrap().len(),2048);
  assert_eq!(store.since(1)["gap"],true);assert_eq!(store.since(2049)["events"].as_array().unwrap().len(),1);
 }
}
