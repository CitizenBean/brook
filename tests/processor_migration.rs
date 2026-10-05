mod common;
use brook::*;
use common::*;
use rusqlite::{params, Connection};
#[test]
fn migrates_v1_without_changing_request_identity_or_ready_context() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (session, lease, input) = setup(&mut s);
    let receipt = ready(&mut s, &lease, &input);
    drop(s);
    let db = Connection::open(dir.path().join("brook.sqlite3")).unwrap();
    let canonical: String = db
        .query_row(
            "SELECT canonical FROM requests WHERE id=?",
            [receipt.request],
            |r| r.get(0),
        )
        .unwrap();
    let context: String = db
        .query_row(
            "SELECT context FROM jobs WHERE request=?",
            [receipt.request],
            |r| r.get(0),
        )
        .unwrap();
    // Rebuild the exact published schema around its persisted data; no processing tables.
    db.execute_batch("DROP TABLE processing_ingress; DROP TABLE processing_deliveries; DROP TABLE processing_state; DROP TABLE processing_graphs; DROP TABLE terminal_bindings; DROP TABLE processing_meta; UPDATE meta SET schema_version=1;").unwrap();
    let pretty = serde_json::to_string_pretty(
        &serde_json::from_str::<serde_json::Value>(&canonical).unwrap(),
    )
    .unwrap();
    db.execute(
        "UPDATE requests SET canonical=? WHERE id=?",
        params![pretty, receipt.request],
    )
    .unwrap();
    drop(db);
    let mut s = store(dir.path());
    let lease = s.claim(session, "after-migration", 1000).unwrap();
    assert_eq!(s.admit(&lease, &input).unwrap(), receipt);
    let attempt = s.claim_job(&lease, receipt).unwrap();
    s.complete_job(&attempt, "result").unwrap();
    drop(s);
    let db = Connection::open(dir.path().join("brook.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT schema_version FROM meta", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row("SELECT canonical FROM requests", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        pretty
    );
    assert_eq!(
        db.query_row("SELECT context FROM jobs", [], |r| r.get::<_, String>(0))
            .unwrap(),
        context
    );
}
#[test]
fn original_empty_schema_migrates_and_failed_migration_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("brook.sqlite3")).unwrap();
    db.execute_batch(include_str!("fixtures/processor-base-v1.sql"))
        .unwrap();
    db.execute(
        "INSERT INTO meta VALUES(1,1,'fixture-store',0,0,?)",
        [serde_json::to_string(&Limits::default()).unwrap()],
    )
    .unwrap();
    drop(db);
    let wrong = Limits {
        requests: 1,
        ..Default::default()
    };
    assert!(Store::open(
        dir.path(),
        wrong,
        std::sync::Arc::new(ManualClock::default())
    )
    .is_err());
    let db = Connection::open(dir.path().join("brook.sqlite3")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='processing_deliveries'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT schema_version FROM meta", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(db);
    drop(store(dir.path()));
}
#[test]
fn future_schema_rejected_before_ddl() {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("brook.sqlite3")).unwrap();
    db.execute_batch("CREATE TABLE meta(id INTEGER PRIMARY KEY,schema_version INTEGER); INSERT INTO meta VALUES(1,999);").unwrap();
    drop(db);
    assert!(Store::open(
        dir.path(),
        Limits::default(),
        std::sync::Arc::new(ManualClock::default())
    )
    .is_err());
    let db = Connection::open(dir.path().join("brook.sqlite3")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn unsupported_processing_layout_does_not_advance_incarnation() {
    let dir = tempfile::tempdir().unwrap();
    drop(store(dir.path()));
    let db = Connection::open(dir.path().join("brook.sqlite3")).unwrap();
    db.execute("INSERT INTO processing_meta VALUES(1,999,'{}')", [])
        .unwrap();
    let old: i64 = db
        .query_row("SELECT incarnation FROM meta", [], |r| r.get(0))
        .unwrap();
    drop(db);
    assert!(Store::open(
        dir.path(),
        Limits::default(),
        std::sync::Arc::new(ManualClock::default())
    )
    .is_err());
    let db = Connection::open(dir.path().join("brook.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT incarnation FROM meta", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        old
    );
}
