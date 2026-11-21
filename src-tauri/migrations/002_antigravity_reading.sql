CREATE TABLE IF NOT EXISTS antigravity_reading (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    source_path TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    payload TEXT NOT NULL
);
PRAGMA user_version = 2;
