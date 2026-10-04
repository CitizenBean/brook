#[path = "support/simulation.rs"]
mod simulation;
use simulation::*;
#[test]
fn seeded_host_composition_matrix() {
    for seed in 0..32 {
        let d = tempfile::tempdir().unwrap();
        let trace = run(d.path(), plan(seed), None).unwrap();
        assert!(trace.failure.is_none(), "seed {seed}: {:?}", trace.failure);
        verify(&trace, true).unwrap();
    }
}
#[test]
fn replay_uses_recorded_actions_and_reproduces_observations() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let trace = run(a.path(), plan(17), None).unwrap();
    assert!(trace.failure.is_none(), "{:?}", trace.failure);
    let replay = run(b.path(), trace.plan.clone(), None).unwrap();
    assert_eq!(
        serde_json::to_value(trace).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
}
#[test]
fn oracle_rejects_known_bad_observations() {
    let d = tempfile::tempdir().unwrap();
    let good = run(d.path(), plan(0), None).unwrap();
    verify(&good, true).unwrap();
    let mut bad = good.clone();
    let last = bad.frames.last_mut().unwrap();
    last.observed.effects.push(last.observed.effects[0].clone());
    assert!(verify(&bad, true)
        .unwrap_err()
        .to_string()
        .contains("effect repeated"));
    let mut bad = good.clone();
    let sid = bad.works[0].session.0;
    bad.frames
        .last_mut()
        .unwrap()
        .observed
        .histories
        .get_mut(&sid)
        .unwrap()
        .remove(0);
    assert!(verify(&bad, true)
        .unwrap_err()
        .to_string()
        .contains("history rewound"));
    let mut bad = good.clone();
    bad.frames.last_mut().unwrap().observed.requests[4].outbox = "sent".into();
    assert!(verify(&bad, true)
        .unwrap_err()
        .to_string()
        .contains("unknown async"));
    let mut bad = good.clone();
    let sid = bad.works[0].session.0;
    let event = bad.frames.last().unwrap().observed.histories[&sid]
        .iter()
        .find(|e| e.kind == "result")
        .unwrap()
        .clone();
    bad.frames
        .last_mut()
        .unwrap()
        .observed
        .histories
        .get_mut(&sid)
        .unwrap()
        .push(event);
    assert!(verify(&bad, true)
        .unwrap_err()
        .to_string()
        .contains("duplicate logical"));
}

#[test]
fn oracle_detects_missing_children_reset_attempts_and_context_leaks() {
    let d = tempfile::tempdir().unwrap();
    let good = run(d.path(), plan(2), None).unwrap();
    verify(&good, true).unwrap();
    let mut bad = good.clone();
    bad.frames
        .last_mut()
        .unwrap()
        .observed
        .deliveries
        .iter_mut()
        .find(|d| d.status == "done")
        .unwrap()
        .children
        .clear();
    assert!(verify(&bad, true)
        .unwrap_err()
        .to_string()
        .contains("missing configured child"));
    let mut bad = good.clone();
    let id = bad.works[0].delivery;
    bad.frames
        .last_mut()
        .unwrap()
        .observed
        .deliveries
        .iter_mut()
        .find(|d| d.id == id)
        .unwrap()
        .attempts = 0;
    assert!(verify(&bad, true)
        .unwrap_err()
        .to_string()
        .contains("attempt budget reset"));
    let mut bad = good.clone();
    let foreign = bad.works[3].session;
    bad.frames
        .last_mut()
        .unwrap()
        .observed
        .invocations
        .iter_mut()
        .find(|i| i.request == bad.works[0].receipt.request)
        .unwrap()
        .context
        .session = foreign;
    assert!(verify(&bad, true)
        .unwrap_err()
        .to_string()
        .contains("cross-session context"));
}
#[test]
fn inserting_identical_retries_does_not_change_final_observations() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let original = run(a.path(), plan(9), None).unwrap();
    verify(&original, true).unwrap();
    let mut p = original.plan.clone();
    p.actions = p
        .actions
        .into_iter()
        .flat_map(|a| {
            if matches!(a, Action::Duplicate(_) | Action::DuplicateReply(_)) {
                vec![a.clone(), a.clone(), a]
            } else {
                vec![a]
            }
        })
        .collect();
    let extra = run(b.path(), p, None).unwrap();
    verify(&extra, true).unwrap();
    assert_eq!(
        serde_json::to_value(&original.frames.last().unwrap().observed).unwrap(),
        serde_json::to_value(&extra.frames.last().unwrap().observed).unwrap()
    );
}
#[test]
fn failing_command_persists_replayable_trace_with_last_observation() {
    let d = tempfile::tempdir().unwrap();
    let output = d.path().join("trace.json");
    let p = Plan {
        version: 2,
        seed: 99,
        actions: vec![Action::Admit(0), Action::Reply(999)],
    };
    let t = run(&d.path().join("store"), p, Some(&output)).unwrap();
    assert!(t.failure.as_ref().unwrap().contains("unknown work"));
    let saved: Trace = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(saved.frames.len(), 2);
    let other = tempfile::tempdir().unwrap();
    let replay = run(other.path(), saved.plan, None).unwrap();
    assert_eq!(replay.failure, t.failure);
}

#[test]
fn oracle_rejects_missing_or_misbound_children_and_actual_wrong_destination() {
    let d = tempfile::tempdir().unwrap();
    let good = run(d.path(), plan(3), None).unwrap();
    verify(&good, true).unwrap();
    for fault in ["missing", "parent", "node", "scope", "graph", "destination"] {
        let mut bad = good.clone();
        let snapshot = &mut bad.frames.last_mut().unwrap().observed;
        let parent = snapshot
            .deliveries
            .iter()
            .find(|d| d.scope.node == "filter")
            .unwrap()
            .id;
        let child = snapshot
            .deliveries
            .iter()
            .find(|d| d.id == parent)
            .unwrap()
            .children[0];
        match fault {
            "missing" => snapshot.deliveries.retain(|d| d.id != child),
            "parent" => {
                snapshot
                    .deliveries
                    .iter_mut()
                    .find(|d| d.id == child)
                    .unwrap()
                    .parent = Some(brook::processing::DeliveryId(999999))
            }
            "node" => {
                snapshot
                    .deliveries
                    .iter_mut()
                    .find(|d| d.id == child)
                    .unwrap()
                    .scope
                    .node = "output".into()
            }
            "scope" => {
                snapshot
                    .deliveries
                    .iter_mut()
                    .find(|d| d.id == child)
                    .unwrap()
                    .scope
                    .key = "terminal:wrong".into()
            }
            "graph" => {
                snapshot
                    .deliveries
                    .iter_mut()
                    .find(|d| d.id == child)
                    .unwrap()
                    .graph_version = 2
            }
            "destination" => {
                snapshot
                    .tool_effects
                    .values_mut()
                    .next()
                    .unwrap()
                    .destination
                    .recipient = "wrong-target".into()
            }
            _ => unreachable!(),
        }
        let error = verify(&bad, true).unwrap_err().to_string();
        let expected = match fault {
            "missing" => "child record missing",
            "parent" => "child parent identity",
            "node" => "unexpected node",
            "scope" => "scope or graph",
            "graph" => "graph",
            "destination" => "destination violated",
            _ => unreachable!(),
        };
        assert!(error.contains(expected), "fault {fault}: {error}");
    }
}

#[test]
fn observation_failure_preserves_original_action_error_and_last_good_snapshot() {
    let d = tempfile::tempdir().unwrap();
    let output = d.path().join("failure.json");
    let mut calls = 0;
    let trace = run_with_observer(
        &d.path().join("store"),
        Plan {
            version: 2,
            seed: 19,
            actions: vec![Action::Admit(0), Action::Reply(999)],
        },
        Some(&output),
        |driver| {
            calls += 1;
            if calls == 2 {
                Err("injected snapshot failure".into())
            } else {
                driver.snapshot()
            }
        },
    )
    .unwrap();
    assert!(trace
        .failure
        .as_ref()
        .unwrap()
        .contains("unknown work script ID"));
    assert!(!trace
        .failure
        .as_ref()
        .unwrap()
        .contains("injected snapshot"));
    assert!(trace.frames[1]
        .observation_failure
        .as_ref()
        .unwrap()
        .contains("injected snapshot failure"));
    assert_eq!(
        serde_json::to_value(&trace.frames[0].observed).unwrap(),
        serde_json::to_value(&trace.frames[1].observed).unwrap()
    );
    let saved: Trace = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(saved.failure, trace.failure);
    assert_eq!(
        saved.frames[1].observation_failure,
        trace.frames[1].observation_failure
    );
}
#[test]
fn oracle_checks_complete_context_identity_for_every_harness() {
    let d = tempfile::tempdir().unwrap();
    let good = run(d.path(), plan(7), None).unwrap();
    verify(&good, true).unwrap();
    for harness in ["scripted-simulation-v1", "echo-v1"] {
        for field in [
            "request",
            "agent",
            "instruction",
            "payload",
            "causal",
            "outcome",
        ] {
            let mut bad = good.clone();
            let context = &mut bad
                .frames
                .last_mut()
                .unwrap()
                .observed
                .invocations
                .iter_mut()
                .find(|i| i.harness == harness)
                .unwrap()
                .context;
            match field {
                "request" => context.request += 999,
                "agent" => context.agent.role = "wrong-role".into(),
                "instruction" => context.instruction = "wrong instruction".into(),
                "payload" => context.payload = "wrong payload".into(),
                "causal" => context.causal_events.clear(),
                "outcome" => context.outcome = "wrong reply".into(),
                _ => unreachable!(),
            };
            let error = verify(&bad, true).unwrap_err().to_string();
            assert!(error.contains("context"), "{harness}/{field}: {error}");
        }
    }
}
#[test]
fn oracle_rejects_wrong_child_ids_orphans_and_all_target_fields() {
    let d = tempfile::tempdir().unwrap();
    let good = run(d.path(), plan(5), None).unwrap();
    verify(&good, true).unwrap();
    for fault in [
        "child-id",
        "orphan",
        "account",
        "recipient",
        "payload",
        "effect-id",
    ] {
        let mut bad = good.clone();
        let snapshot = &mut bad.frames.last_mut().unwrap().observed;
        match fault {
            "child-id" => {
                snapshot
                    .deliveries
                    .iter_mut()
                    .find(|d| d.scope.node == "filter")
                    .unwrap()
                    .children[0] = brook::processing::DeliveryId(777777)
            }
            "orphan" => {
                let mut orphan = snapshot
                    .deliveries
                    .iter()
                    .find(|d| d.scope.node == "output")
                    .unwrap()
                    .clone();
                orphan.id = brook::processing::DeliveryId(888888);
                snapshot.deliveries.push(orphan);
            }
            "account" => {
                snapshot
                    .tool_effects
                    .values_mut()
                    .next()
                    .unwrap()
                    .destination
                    .account = "wrong-account".into()
            }
            "recipient" => {
                snapshot
                    .tool_effects
                    .values_mut()
                    .next()
                    .unwrap()
                    .destination
                    .recipient = "wrong-recipient".into()
            }
            "payload" => {
                snapshot.tool_effects.values_mut().next().unwrap().payload = "wrong payload".into()
            }
            "effect-id" => snapshot.tool_effects.values_mut().next().unwrap().request = 123456,
            _ => unreachable!(),
        }
        let error = verify(&bad, true).unwrap_err().to_string();
        let expected = match fault {
            "child-id" => "child record missing",
            "orphan" => "absent from parent",
            "account" | "recipient" => "destination violated",
            "payload" => "payload violated",
            "effect-id" => "request identity mismatch",
            _ => unreachable!(),
        };
        assert!(error.contains(expected), "{fault}: {error}");
    }
}
