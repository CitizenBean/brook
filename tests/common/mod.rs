#![allow(dead_code)]
use brook::*;
use std::sync::Arc;
pub const PEER: &str = "trusted-fake-peer";
pub fn target() -> Destination {
    Destination {
        sink: "fake".into(),
        account: "account-a".into(),
        recipient: "recipient-a".into(),
    }
}
pub fn setup(store: &mut Store) -> (SessionId, Lease, Submission) {
    let session = store
        .resolve_session("namespace-a", "client-a", "conversation")
        .unwrap();
    let lease = store.claim(session, "worker", 10000).unwrap();
    let event = store.message(session, "original instruction").unwrap();
    let version = store
        .grant("grant-a", session, &target(), "work", 10000)
        .unwrap();
    let input = Submission {
        agent: AgentConfig::default(),
        operation: "op-a".into(),
        grant: "grant-a".into(),
        grant_version: version,
        destination: target(),
        instruction: "await work".into(),
        payload: "work".into(),
        expected_peer: PEER.into(),
        causal_events: vec![event],
        lifetime_ms: 9000,
    };
    (session, lease, input)
}
pub fn store(path: &std::path::Path) -> Store {
    Store::open(path, Limits::default(), Arc::new(ManualClock::default())).unwrap()
}
pub fn sent(store: &mut Store, lease: &Lease, input: &Submission) -> Receipt {
    let receipt = store.admit(lease, input).unwrap();
    let attempt = store.begin_send(lease, receipt).unwrap();
    store.finish_send(&attempt, SendOutcome::Applied).unwrap();
    receipt
}
pub fn accepted(store: &mut Store, lease: &Lease, input: &Submission) -> Receipt {
    let receipt = sent(store, lease, input);
    store.accept_reply(receipt, PEER, "answer").unwrap();
    receipt
}
pub fn ready(store: &mut Store, lease: &Lease, input: &Submission) -> Receipt {
    let receipt = accepted(store, lease, input);
    let context = store.build_context(receipt).unwrap();
    store.admit_resume(lease, &context).unwrap();
    receipt
}
