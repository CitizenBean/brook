use brook::{processing::*, *};
use serde_json::{json, Value};
use std::sync::Arc;

fn setup(l: ProcessingLimits) -> (tempfile::TempDir, Store, TerminalClient, Graph) {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(
        d.path(),
        Limits::default(),
        Arc::new(ManualClock::default()),
    )
    .unwrap();
    s.configure_processing(l).unwrap();
    let i = TerminalIdentity::local();
    let c = s.terminal_client(i.clone()).unwrap();
    let b = Version::new("stdout", 1);
    s.bind_terminal(&b, &c).unwrap();
    let g = terminal_recipe(&i, b).unwrap();
    s.register_graph(&g).unwrap();
    (d, s, c, g)
}
fn input(s: &mut Store, c: &TerminalClient, op: &str) -> DeliveryId {
    s.submit_terminal(c, "terminal", 1, op, json!("hello"))
        .unwrap()
}
fn attempt(s: &mut Store, id: DeliveryId) -> (ProcessingLease, ProcessingAttempt) {
    let l = s.claim_processing(id, "worker", 100).unwrap();
    let a = s.prepare_processing(&l, id).unwrap();
    (l, a)
}
fn proposal() -> Proposal {
    Proposal {
        reason: "test configured path".into(),
        state: json!(1),
        outputs: vec![json!("hello")],
        routing: Routing::ConfiguredNext,
    }
}

#[test]
fn minimal_recipe_restart_and_transform_override() {
    let (d, mut s, c, mut g) = setup(ProcessingLimits::default());
    let id = input(&mut s, &c, "one");
    let mut out = Vec::new();
    assert_eq!(run_terminal_recipe(&mut s, &c, &mut out).unwrap(), 3);
    assert_eq!(out, b"hello\n");
    let router = g.nodes["route"].code.clone();
    g.version = 2;
    insert_uppercase(&mut g).unwrap();
    assert_eq!(g.nodes["route"].code, router);
    s.register_graph(&g).unwrap();
    let second = s
        .submit_terminal(&c, "terminal", 2, "two", json!("world"))
        .unwrap();
    drop(s);
    let mut s = Store::open(
        d.path(),
        Limits::default(),
        Arc::new(ManualClock::default()),
    )
    .unwrap();
    assert_eq!(
        s.submit_terminal(&c, "terminal", 1, "one", json!("hello"))
            .unwrap(),
        id
    );
    let mut out = Vec::new();
    assert_eq!(run_terminal_recipe(&mut s, &c, &mut out).unwrap(), 4);
    assert_eq!(out, b"WORLD\n");
    assert_eq!(s.inspect_processing(second).unwrap().graph_version, 2);
    assert!(matches!(
        s.submit_terminal(&c, "terminal", 2, "one", json!("hello")),
        Err(Error::Conflict)
    ));
}
#[test]
fn immutable_graph_config_code_schema_and_binding() {
    let (_, mut s, c, mut g) = setup(ProcessingLimits::default());
    let id = input(&mut s, &c, "one");
    let (_, a) = attempt(&mut s, id);
    g.nodes.get_mut("filter").unwrap().config = json!({"new":true});
    assert!(matches!(s.register_graph(&g), Err(Error::Conflict)));
    g.version = 2;
    s.register_graph(&g).unwrap();
    assert_eq!(a.config(), &Value::Null);
    assert!(s
        .execute_processor(&a, &RustAdapter(TextProcessor("uppercase")))
        .is_err());
    let other = s
        .terminal_client(TerminalIdentity {
            conversation: "other".into(),
            ..TerminalIdentity::local()
        })
        .unwrap();
    assert!(matches!(
        s.bind_terminal(&Version::new("stdout", 1), &other),
        Err(Error::Conflict)
    ));
    let mut invalid = g.clone();
    invalid.version = 3;
    invalid
        .nodes
        .get_mut("output")
        .unwrap()
        .input
        .identity
        .version = 2;
    assert!(s.register_graph(&invalid).is_err());
}
#[test]
fn fanout_all_or_nothing_duplicate_and_causal_ids() {
    let (_, mut s, c, mut g) = setup(ProcessingLimits::default());
    g.version = 2;
    let leaf = g.nodes["output"].clone();
    g.nodes.insert("second".into(), leaf);
    let r = g.nodes.get_mut("route").unwrap();
    r.branches.insert("also".into(), vec!["second".into()]);
    g.entry = "route".into();
    s.register_graph(&g).unwrap();
    let id = s
        .submit_terminal(&c, "terminal", 2, "one", json!("hello"))
        .unwrap();
    let (_, a) = attempt(&mut s, id);
    let mut p = proposal();
    p.routing = Routing::SelectedBranches(vec!["display".into(), "also".into()]);
    let done = s.commit_processing(&a, &p).unwrap();
    assert_eq!(done.deliveries.len(), 2);
    assert_eq!(s.commit_processing(&a, &proposal()).unwrap(), done);
    assert_eq!(s.pending_processing(64).unwrap().len(), 2);
    for child in done.deliveries {
        assert_eq!(s.inspect_processing(child).unwrap().parent, Some(id));
    }
}
#[test]
fn invalid_route_schema_state_and_scope_commit_nothing() {
    let (_, mut s, c, _) = setup(ProcessingLimits::default());
    let id = input(&mut s, &c, "one");
    let (_, a) = attempt(&mut s, id);
    for p in [
        Proposal {
            routing: Routing::SelectedBranches(vec!["bad".into()]),
            ..proposal()
        },
        Proposal {
            state: json!({}),
            ..proposal()
        },
        Proposal {
            outputs: vec![json!(7)],
            ..proposal()
        },
        Proposal {
            outputs: vec![],
            ..proposal()
        },
        Proposal {
            reason: String::new(),
            ..proposal()
        },
    ] {
        assert!(s.commit_processing(&a, &p).is_err());
        assert!(s.committed_processing(id).unwrap().is_none());
    }
    let other = s
        .terminal_client(TerminalIdentity {
            conversation: "other".into(),
            ..TerminalIdentity::local()
        })
        .unwrap();
    let second = input(&mut s, &other, "two");
    let (l, _) = attempt(&mut s, second);
    assert!(matches!(
        s.prepare_processing(&l, id),
        Err(Error::Unauthorized)
    ));
    assert_eq!(s.pending_processing(64).unwrap().len(), 0);
}
#[test]
fn route_choice_never_grants_destination_authority() {
    let (_, mut s, c, mut g) = setup(ProcessingLimits::default());
    let other = s
        .terminal_client(TerminalIdentity {
            conversation: "other".into(),
            ..TerminalIdentity::local()
        })
        .unwrap();
    let b = Version::new("other", 1);
    s.bind_terminal(&b, &other).unwrap();
    g.version = 2;
    g.entry = "route".into();
    let mut n = g.nodes["output"].clone();
    n.kind = NodeKind::Terminal { binding: b };
    g.nodes.insert("other".into(), n);
    g.nodes
        .get_mut("route")
        .unwrap()
        .branches
        .insert("bad".into(), vec!["output".into(), "other".into()]);
    s.register_graph(&g).unwrap();
    let id = s
        .submit_terminal(&c, "terminal", 2, "one", json!("hello"))
        .unwrap();
    let (_, a) = attempt(&mut s, id);
    let p = Proposal {
        routing: Routing::SelectedBranches(vec!["bad".into()]),
        ..proposal()
    };
    assert!(matches!(
        s.commit_processing(&a, &p),
        Err(Error::Unauthorized)
    ));
    assert!(s.pending_processing(64).unwrap().is_empty());
    assert!(s.committed_processing(id).unwrap().is_none());
    let p = Proposal {
        routing: Routing::SelectedBranches(vec!["missing".into()]),
        ..proposal()
    };
    assert!(s.commit_processing(&a, &p).is_err());
}
#[test]
fn state_conflict_retries_are_bounded_and_independent_keys_work() {
    let (_, mut s, c, _) = setup(ProcessingLimits {
        attempts: 2,
        ..Default::default()
    });
    let first = input(&mut s, &c, "one");
    let second = input(&mut s, &c, "two");
    let (l, a) = attempt(&mut s, first);
    let b = s.prepare_processing(&l, second).unwrap();
    s.commit_processing(&a, &proposal()).unwrap();
    assert!(matches!(
        s.commit_processing(&b, &proposal()),
        Err(Error::RevisionChanged)
    ));
    let b = s.prepare_processing(&l, second).unwrap();
    assert_eq!(b.state(), &json!(1));
    s.fail_processing(&b, "retry", true).unwrap();
    assert_eq!(s.inspect_processing(second).unwrap().status, "failed");
    let other = s
        .terminal_client(TerminalIdentity {
            conversation: "other".into(),
            ..TerminalIdentity::local()
        })
        .unwrap();
    let id = input(&mut s, &other, "one");
    let (_, a) = attempt(&mut s, id);
    assert_eq!(a.state(), &Value::Null);
    s.commit_processing(&a, &proposal()).unwrap();
}
#[test]
fn expired_and_restarted_owners_cannot_commit_or_renew() {
    let d = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::default());
    let mut s = Store::open(d.path(), Limits::default(), clock.clone()).unwrap();
    s.configure_processing(Default::default()).unwrap();
    let i = TerminalIdentity::local();
    let c = s.terminal_client(i.clone()).unwrap();
    s.bind_terminal(&Version::new("stdout", 1), &c).unwrap();
    s.register_graph(&terminal_recipe(&i, Version::new("stdout", 1)).unwrap())
        .unwrap();
    let id = input(&mut s, &c, "one");
    let (l, a) = attempt(&mut s, id);
    clock.set(100);
    assert!(matches!(
        s.heartbeat_processing(&l, 100),
        Err(Error::Fenced)
    ));
    let new = s.claim_processing(id, "new", 100).unwrap();
    assert!(matches!(
        s.commit_processing(&a, &proposal()),
        Err(Error::Fenced)
    ));
    let a = s.prepare_processing(&new, id).unwrap();
    drop(s);
    let mut s = Store::open(
        d.path(),
        Limits::default(),
        Arc::new(ManualClock::default()),
    )
    .unwrap();
    assert!(matches!(
        s.commit_processing(&a, &proposal()),
        Err(Error::Fenced)
    ));
}
#[test]
fn quotas_preserve_failure_capacity_and_suppression_is_explicit() {
    let (_, mut s, c, _) = setup(ProcessingLimits {
        deliveries: 1,
        ..Default::default()
    });
    let id = input(&mut s, &c, "one");
    let (_, a) = attempt(&mut s, id);
    assert!(matches!(
        s.commit_processing(&a, &proposal()),
        Err(Error::Capacity)
    ));
    s.fail_processing(&a, "fanout exceeds backlog", false)
        .unwrap();
    assert_eq!(s.inspect_processing(id).unwrap().status, "failed");
    let (_, mut s, c, _) = setup(Default::default());
    let id = s
        .submit_terminal(&c, "terminal", 1, "blank", json!(""))
        .unwrap();
    run_terminal_recipe(&mut s, &c, &mut Vec::new()).unwrap();
    let o = s.committed_processing(id).unwrap().unwrap();
    assert!(o.deliveries.is_empty());
    assert_eq!(o.decision.routing, Routing::Suppress("blank input".into()));
}
#[test]
fn terminal_revocation_uncertainty_and_no_blind_reprint() {
    let (d, mut s, c, mut g) = setup(Default::default());
    g.version = 2;
    g.entry = "output".into();
    s.register_graph(&g).unwrap();
    let id = s
        .submit_terminal(&c, "terminal", 2, "one", json!("hello"))
        .unwrap();
    let l = s.claim_processing(id, "writer", 100).unwrap();
    s.begin_terminal(&l, id, &c).unwrap();
    drop(s);
    let mut s = Store::open(
        d.path(),
        Limits::default(),
        Arc::new(ManualClock::default()),
    )
    .unwrap();
    assert_eq!(s.inspect_processing(id).unwrap().status, "unknown");
    let l = s.claim_processing(id, "writer", 100).unwrap();
    assert!(matches!(s.begin_terminal(&l, id, &c), Err(Error::NotReady)));
    s.release_processing(&l).unwrap();
    let id = s
        .submit_terminal(&c, "terminal", 2, "two", json!("hello"))
        .unwrap();
    let l = s.claim_processing(id, "writer", 100).unwrap();
    s.revoke_terminal(&Version::new("stdout", 1)).unwrap();
    assert!(matches!(
        s.begin_terminal(&l, id, &c),
        Err(Error::Unauthorized)
    ));
}
#[test]
fn dags_schema_changes_and_cross_namespace_rejected() {
    let (_, mut s, _, mut g) = setup(Default::default());
    g.version = 2;
    g.nodes.get_mut("filter").unwrap().next = vec!["filter".into()];
    assert!(s.register_graph(&g).is_err());
    g.nodes.get_mut("filter").unwrap().next = vec!["route".into()];
    g.namespace = "elsewhere".into();
    assert!(matches!(s.register_graph(&g), Err(Error::Unauthorized)));
}
#[test]
fn input_output_state_budgets_count_serialized_bytes() {
    let (_, mut s, c, _) = setup(ProcessingLimits {
        input_bytes: 128,
        state_bytes: 64,
        output_bytes: 256,
        ..Default::default()
    });
    assert!(matches!(
        s.submit_terminal(&c, "terminal", 1, "big", json!("🦀".repeat(100))),
        Err(Error::Capacity)
    ));
    let id = input(&mut s, &c, "one");
    let (_, a) = attempt(&mut s, id);
    let p = Proposal {
        outputs: vec![json!("x".repeat(256))],
        ..proposal()
    };
    assert!(matches!(s.commit_processing(&a, &p), Err(Error::Capacity)));
    assert!(s.committed_processing(id).unwrap().is_none());
}

#[test]
fn routing_operator_can_transform_and_chains_never_return_to_dispatcher() {
    let (_, mut s, c, mut g) = setup(Default::default());
    g.version = 2;
    g.entry = "route".into();
    insert_uppercase(&mut g).unwrap();
    s.register_graph(&g).unwrap();
    let id = s
        .submit_terminal(&c, "terminal", 2, "one", json!("source"))
        .unwrap();
    let (_, a) = attempt(&mut s, id);
    let p = Proposal {
        outputs: vec![json!("operator transformed")],
        routing: Routing::SelectedBranches(vec!["display".into()]),
        ..proposal()
    };
    let done = s.commit_processing(&a, &p).unwrap();
    let id = done.deliveries[0];
    assert_eq!(s.inspect_processing(id).unwrap().scope.node, "uppercase");
    let (_, a) = attempt(&mut s, id);
    assert_eq!(a.event().payload, json!("operator transformed"));
    let p = s
        .execute_processor(&a, &RustAdapter(TextProcessor("uppercase")))
        .unwrap();
    let done = s.commit_processing(&a, &p).unwrap();
    assert_eq!(
        s.inspect_processing(done.deliveries[0]).unwrap().scope.node,
        "output"
    );
    assert_eq!(s.pending_processing(64).unwrap().len(), 1);
}
#[test]
fn cross_store_tokens_and_terminal_mapping_fail_closed() {
    let (_, mut a, client, _) = setup(Default::default());
    let id = input(&mut a, &client, "one");
    let (lease, attempt) = attempt(&mut a, id);
    let (_, mut b, other, _) = setup(Default::default());
    input(&mut b, &other, "one");
    assert!(matches!(
        b.submit_terminal(&client, "terminal", 1, "x", json!("x")),
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        b.commit_processing(&attempt, &proposal()),
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        b.prepare_processing(&lease, id),
        Err(Error::Fenced)
    ));
}
#[test]
fn fanout_and_snapshot_limits_reject_overproduction() {
    let (_, mut s, c, _) = setup(ProcessingLimits {
        fanout: 2,
        ..Default::default()
    });
    let id = input(&mut s, &c, "one");
    let (_, a) = attempt(&mut s, id);
    let p = Proposal {
        outputs: vec![json!("a"), json!("b"), json!("c")],
        ..proposal()
    };
    assert!(matches!(s.commit_processing(&a, &p), Err(Error::Capacity)));
    s.fail_processing(&a, "overproduction", false).unwrap();
    assert_eq!(s.inspect_processing(id).unwrap().status, "failed");
}
#[test]
fn partial_terminal_write_is_unknown_and_finished_attempt_cannot_print_again() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("partial connection failure"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (_, mut s, c, mut g) = setup(Default::default());
    g.version = 2;
    g.entry = "output".into();
    s.register_graph(&g).unwrap();
    let id = s
        .submit_terminal(&c, "terminal", 2, "one", json!("hello"))
        .unwrap();
    let l = s.claim_processing(id, "writer", 100).unwrap();
    let a = s.begin_terminal(&l, id, &c).unwrap();
    assert!(s.print_terminal(&a, &mut Broken).is_err());
    assert_eq!(s.inspect_processing(id).unwrap().status, "unknown");
    assert!(matches!(
        s.print_terminal(&a, &mut Vec::new()),
        Err(Error::NotReady)
    ));
}
#[test]
fn state_schema_cannot_change_under_an_existing_scope() {
    let (_, mut s, c, mut g) = setup(Default::default());
    input(&mut s, &c, "one");
    g.version = 2;
    let n = g.nodes.get_mut("filter").unwrap();
    n.state_schema = Schema {
        identity: Version::new("new-state", 1),
        shape: JsonShape::Object,
    };
    n.initial_state = json!({});
    s.register_graph(&g).unwrap();
    assert!(s
        .submit_terminal(&c, "terminal", 2, "two", json!("hello"))
        .is_err());
    assert_eq!(s.pending_processing(64).unwrap().len(), 1);
}
#[test]
fn inspection_excludes_payload_state_and_effective_graph_roundtrips() {
    let (_, mut s, c, g) = setup(Default::default());
    let id = s
        .submit_terminal(&c, "terminal", 1, "one", json!("private test content"))
        .unwrap();
    let view = serde_json::to_string(&s.inspect_processing(id).unwrap()).unwrap();
    assert!(!view.contains("private test content"));
    assert_eq!(s.effective_graph("local", "terminal", 1).unwrap(), g);
    assert!(s.pending_processing(65).is_err());
}

#[test]
fn terminal_runner_leaves_another_connections_work_pending() {
    let (_, mut s, c, _) = setup(Default::default());
    let other = s
        .terminal_client(TerminalIdentity {
            conversation: "other".into(),
            ..TerminalIdentity::local()
        })
        .unwrap();
    let other_id = input(&mut s, &other, "other");
    let own = input(&mut s, &c, "own");
    let mut output = Vec::new();
    run_terminal_recipe(&mut s, &c, &mut output).unwrap();
    assert_eq!(output, b"hello\n");
    assert_eq!(s.inspect_processing(own).unwrap().status, "done");
    assert_eq!(s.inspect_processing(other_id).unwrap().status, "pending");
}
#[test]
fn exact_state_and_proposal_byte_limits() {
    let (_, mut s, c, mut g) = setup(ProcessingLimits {
        state_bytes: 64,
        output_bytes: 256,
        ..Default::default()
    });
    g.version = 2;
    let n = g.nodes.get_mut("filter").unwrap();
    n.state_schema = Schema {
        identity: Version::new("object", 1),
        shape: JsonShape::Object,
    };
    n.initial_state = json!({});
    s.register_graph(&g).unwrap();
    let id = s
        .submit_terminal(&c, "terminal", 2, "one", json!("hello"))
        .unwrap();
    let (_, a) = attempt(&mut s, id);
    let mut p = proposal();
    p.state = json!({"v":"x".repeat(56)});
    assert_eq!(serde_json::to_vec(&p.state).unwrap().len(), 64);
    let excess = Proposal {
        state: json!({"v":"x".repeat(57)}),
        ..p.clone()
    };
    assert!(matches!(
        s.commit_processing(&a, &excess),
        Err(Error::Capacity)
    ));
    let overhead = serde_json::to_vec(&p).unwrap().len();
    p.reason.push_str(&"x".repeat(256 - overhead));
    assert_eq!(serde_json::to_vec(&p).unwrap().len(), 256);
    let excess = Proposal {
        reason: format!("{}x", p.reason),
        ..p.clone()
    };
    assert!(matches!(
        s.commit_processing(&a, &excess),
        Err(Error::Capacity)
    ));
    s.commit_processing(&a, &p).unwrap();
}

#[test]
fn successful_write_with_failed_finalization_cannot_write_again() {
    let (dir, mut s, c, mut g) = setup(Default::default());
    g.version = 2;
    g.entry = "output".into();
    s.register_graph(&g).unwrap();
    let id = s
        .submit_terminal(&c, "terminal", 2, "one", json!("hello"))
        .unwrap();
    let lease = s.claim_processing(id, "writer", 100).unwrap();
    let a = s.begin_terminal(&lease, id, &c).unwrap();
    let duplicate = a.clone();
    let db = rusqlite::Connection::open(dir.path().join("brook.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_terminal_finish BEFORE UPDATE OF status ON processing_deliveries WHEN NEW.status='printed' BEGIN SELECT RAISE(ABORT,'injected finalization failure'); END;").unwrap();
    let mut output = Vec::new();
    assert!(matches!(
        s.print_terminal(&a, &mut output),
        Err(Error::Sql(_))
    ));
    assert_eq!(output, b"hello\n");
    assert_eq!(s.inspect_processing(id).unwrap().status, "write_started");
    db.execute_batch("DROP TRIGGER fail_terminal_finish")
        .unwrap();
    assert!(matches!(
        s.print_terminal(&duplicate, &mut output),
        Err(Error::NotReady)
    ));
    assert_eq!(output, b"hello\n");
    // Retrying only durable finalization with known evidence does not repeat I/O.
    s.finish_terminal(&a, SendOutcome::Applied).unwrap();
    assert_eq!(s.inspect_processing(id).unwrap().status, "printed");
}

#[test]
fn lease_expiry_during_successful_write_preserves_uncertainty() {
    struct ExpiringWriter {
        clock: Arc<ManualClock>,
        bytes: Vec<u8>,
    }
    impl std::io::Write for ExpiringWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.clock.set(100);
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::default());
    let mut s = Store::open(dir.path(), Limits::default(), clock.clone()).unwrap();
    s.configure_processing(Default::default()).unwrap();
    let identity = TerminalIdentity::local();
    let c = s.terminal_client(identity.clone()).unwrap();
    let binding = Version::new("stdout", 1);
    s.bind_terminal(&binding, &c).unwrap();
    let g = PipelineBuilder::terminal(&identity, binding).build();
    s.register_graph(&g).unwrap();
    let id = s
        .submit_terminal(&c, "terminal", 1, "one", json!("hello"))
        .unwrap();
    let lease = s.claim_processing(id, "writer", 100).unwrap();
    let a = s.begin_terminal(&lease, id, &c).unwrap();
    let mut writer = ExpiringWriter {
        clock,
        bytes: Vec::new(),
    };
    assert!(matches!(
        s.print_terminal(&a, &mut writer),
        Err(Error::Fenced)
    ));
    assert_eq!(s.inspect_processing(id).unwrap().status, "write_started");
    assert!(matches!(
        s.print_terminal(&a.clone(), &mut writer),
        Err(Error::Fenced)
    ));
    assert_eq!(writer.bytes, b"hello\n");
    let lease = s.claim_processing(id, "new-owner", 100).unwrap();
    assert_eq!(s.inspect_processing(id).unwrap().status, "unknown");
    assert!(matches!(
        s.begin_terminal(&lease, id, &c),
        Err(Error::NotReady)
    ));
}

#[test]
fn terminal_escapes_controls_but_preserves_unicode_newlines_and_tabs() {
    let (_, mut s, c, mut g) = setup(Default::default());
    g.version = 2;
    g.entry = "output".into();
    s.register_graph(&g).unwrap();
    let cases = [
        (
            "\u{1b}]52;c;Y2xpcGJvYXJk\u{7}",
            "\\u{1b}]52;c;Y2xpcGJvYXJk\\u{7}\n",
        ),
        (
            "\u{1b}[2J\u{1b}[H\u{1b}[8A",
            "\\u{1b}[2J\\u{1b}[H\\u{1b}[8A\n",
        ),
        (
            "a\u{1b}b\0\r\u{8}\u{7f}\u{9b}2J",
            "a\\u{1b}b\\u{0}\\u{d}\\u{8}\\u{7f}\\u{9b}2J\n",
        ),
        (
            "Hello café 🦀\nnext\tcolumn",
            "Hello café 🦀\nnext\tcolumn\n",
        ),
        ("left\u{202e}right", "left\\u{202e}right\n"),
    ];
    for (n, (input, expected)) in cases.into_iter().enumerate() {
        let id = s
            .submit_terminal(&c, "terminal", 2, &format!("case-{n}"), json!(input))
            .unwrap();
        let lease = s.claim_processing(id, "writer", 100).unwrap();
        let a = s.begin_terminal(&lease, id, &c).unwrap();
        let mut bytes = Vec::new();
        s.print_terminal(&a, &mut bytes).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), expected);
        s.release_processing(&lease).unwrap();
    }
}

#[test]
fn exhausted_recovered_delivery_releases_scope_for_the_next_input() {
    let (dir, mut s, c, _) = setup(ProcessingLimits {
        attempts: 1,
        ..Default::default()
    });
    let first = input(&mut s, &c, "one");
    let second = input(&mut s, &c, "two");
    let (_, _) = attempt(&mut s, first);
    drop(s);
    let mut s = Store::open(
        dir.path(),
        Limits::default(),
        Arc::new(ManualClock::default()),
    )
    .unwrap();
    assert!(matches!(
        run_terminal_recipe(&mut s, &c, &mut Vec::new()),
        Err(Error::Capacity)
    ));
    assert_eq!(s.inspect_processing(first).unwrap().status, "failed");
    let mut output = Vec::new();
    run_terminal_recipe(&mut s, &c, &mut output).unwrap();
    assert_eq!(s.inspect_processing(second).unwrap().status, "done");
    assert_eq!(output, b"hello\n");
}
