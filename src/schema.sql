CREATE TABLE IF NOT EXISTS meta (
 id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL,
 store_id TEXT NOT NULL, incarnation INTEGER NOT NULL, tick INTEGER NOT NULL, limits TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
 id INTEGER PRIMARY KEY AUTOINCREMENT, namespace TEXT NOT NULL, client TEXT NOT NULL,
 external_id TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 0,
 cancellation INTEGER NOT NULL DEFAULT 0, generation INTEGER NOT NULL DEFAULT 0,
 owner TEXT, deadline INTEGER NOT NULL DEFAULT 0, UNIQUE(namespace,client,external_id)
);
CREATE TABLE IF NOT EXISTS grants (
 id TEXT PRIMARY KEY, session INTEGER NOT NULL REFERENCES sessions(id),
 version INTEGER NOT NULL, destination TEXT NOT NULL, content TEXT NOT NULL,
 expires INTEGER NOT NULL, active INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS requests (
 id INTEGER PRIMARY KEY AUTOINCREMENT, session INTEGER NOT NULL REFERENCES sessions(id),
 operation TEXT NOT NULL, canonical TEXT NOT NULL, cancellation INTEGER NOT NULL,
 deadline INTEGER NOT NULL, state TEXT NOT NULL, outcome TEXT,
 reserved_bytes INTEGER NOT NULL, UNIQUE(session,operation)
);
CREATE TABLE IF NOT EXISTS events (
 id INTEGER PRIMARY KEY AUTOINCREMENT, session INTEGER NOT NULL REFERENCES sessions(id),
 revision INTEGER NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL,
 request INTEGER REFERENCES requests(id), UNIQUE(session,revision), UNIQUE(request,kind)
);
CREATE TABLE IF NOT EXISTS pins (
 request INTEGER NOT NULL REFERENCES requests(id), event INTEGER NOT NULL REFERENCES events(id),
 PRIMARY KEY(request,event)
);
CREATE TABLE IF NOT EXISTS outbox (
 request INTEGER PRIMARY KEY REFERENCES requests(id), state TEXT NOT NULL,
 incarnation INTEGER, generation INTEGER, worker TEXT
);
CREATE TABLE IF NOT EXISTS jobs (
 request INTEGER PRIMARY KEY REFERENCES requests(id), state TEXT NOT NULL,
 context TEXT, attempts INTEGER NOT NULL DEFAULT 0,
 incarnation INTEGER, generation INTEGER, worker TEXT, result TEXT
);
CREATE TABLE IF NOT EXISTS recovery (
 request INTEGER PRIMARY KEY REFERENCES requests(id), classification TEXT NOT NULL,
 notified INTEGER NOT NULL DEFAULT 0 CHECK(notified IN (0,1))
);
CREATE INDEX IF NOT EXISTS events_session ON events(session,revision);
