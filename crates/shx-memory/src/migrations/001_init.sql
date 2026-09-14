-- 001_init.sql → user_version = 1
-- CREATE IF NOT EXISTS so a v0 fixture with rows can migrate in place.

CREATE TABLE IF NOT EXISTS interactions (
  id            INTEGER PRIMARY KEY,
  ts            INTEGER NOT NULL,
  session_id    TEXT    NOT NULL,
  project_id    TEXT,
  cwd           TEXT    NOT NULL,
  os            TEXT    NOT NULL,
  shell         TEXT    NOT NULL,
  input_nl      TEXT    NOT NULL,
  output_cmd    TEXT    NOT NULL,
  explanation   TEXT,
  backend       TEXT    NOT NULL,
  model         TEXT    NOT NULL,
  confidence    REAL,
  latency_ms    INTEGER NOT NULL,
  risk_level    TEXT    NOT NULL,
  risk_notes    TEXT,
  from_cache    INTEGER NOT NULL DEFAULT 0,
  accepted      INTEGER,
  executed      INTEGER,
  tags          TEXT
);
CREATE INDEX IF NOT EXISTS idx_interactions_ts        ON interactions(ts DESC);
CREATE INDEX IF NOT EXISTS idx_interactions_project   ON interactions(project_id, ts DESC);
CREATE INDEX IF NOT EXISTS idx_interactions_cache     ON interactions(input_nl, project_id, cwd);

CREATE TABLE IF NOT EXISTS vocabulary (
  term          TEXT NOT NULL,
  expansion     TEXT NOT NULL,
  weight        REAL NOT NULL DEFAULT 1.0,
  source        TEXT NOT NULL,
  last_used_ts  INTEGER NOT NULL,
  use_count     INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (term, expansion)
);

CREATE TABLE IF NOT EXISTS snippets (
  id            INTEGER PRIMARY KEY,
  name          TEXT NOT NULL UNIQUE,
  command       TEXT NOT NULL,
  description   TEXT,
  created_ts    INTEGER NOT NULL,
  use_count     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS feedback (
  id             INTEGER PRIMARY KEY,
  interaction_id INTEGER NOT NULL REFERENCES interactions(id) ON DELETE CASCADE,
  verdict        TEXT NOT NULL,
  note           TEXT,
  ts             INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS shell_history (
  id        INTEGER PRIMARY KEY,
  ts        INTEGER NOT NULL,
  cwd       TEXT,
  cmd       TEXT NOT NULL,
  exit_code INTEGER,
  source    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_shell_history_ts ON shell_history(ts DESC);

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '1');
