CREATE TABLE IF NOT EXISTS claude_reading (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    observed_at INTEGER NOT NULL,
    payload TEXT NOT NULL
);
PRAGMA user_version = 3;
