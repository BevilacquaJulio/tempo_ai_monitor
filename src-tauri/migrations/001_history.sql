CREATE TABLE IF NOT EXISTS sessions (
    provider_id TEXT NOT NULL,
    external_id TEXT NOT NULL,
    last_activity INTEGER NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY (provider_id, external_id)
);
CREATE INDEX IF NOT EXISTS sessions_activity ON sessions(last_activity DESC);
CREATE TABLE IF NOT EXISTS history_reset (id INTEGER PRIMARY KEY CHECK (id = 1), cleared_at INTEGER NOT NULL);

CREATE TABLE IF NOT EXISTS alert_deliveries (
    provider_id TEXT NOT NULL,
    window_id TEXT NOT NULL,
    resets_at INTEGER NOT NULL,
    threshold INTEGER NOT NULL,
    PRIMARY KEY (provider_id, window_id, resets_at, threshold)
);
PRAGMA user_version = 1;
