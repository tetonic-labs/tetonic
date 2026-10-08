//! CODE-02 IMPLEMENT source pins. Distinctive `009` pins are run-shaped keys.
//! Distinctive `025` pins are the `turn.is_some()` incremental wrap.

#[test]
fn code02_begin_job_run_task_id_is_run_not_session() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::RootKeys);
}

#[test]
fn code02_begin_job_run_delivery_key_is_run_not_session() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::RootKeys);
}

#[test]
fn code02_sessionless_still_task_root_run() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;
