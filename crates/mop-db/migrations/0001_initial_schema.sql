-- mop SQLite Schema v1

CREATE TABLE IF NOT EXISTS users (
  id            TEXT PRIMARY KEY,
  username      TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  role          TEXT NOT NULL CHECK (role IN ('admin','operator','viewer')),
  disabled      INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

-- tower-sessions table
CREATE TABLE IF NOT EXISTS sessions (
  id            TEXT PRIMARY KEY,
  data          BLOB NOT NULL,
  expiry_date   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_expiry ON sessions(expiry_date);

CREATE TABLE IF NOT EXISTS tower_sessions (
  id            TEXT PRIMARY KEY,
  data          BLOB NOT NULL,
  expiry_date   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tower_sessions_expiry ON tower_sessions(expiry_date);

CREATE TABLE IF NOT EXISTS audit_events (
  id            TEXT PRIMARY KEY,
  ts            TEXT NOT NULL,
  user_id       TEXT,
  username      TEXT,
  action        TEXT NOT NULL,
  resource_kind TEXT,
  resource_id   TEXT,
  detail_json   TEXT,
  result        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_events_ts ON audit_events(ts);

CREATE TABLE IF NOT EXISTS resources (
  id            TEXT PRIMARY KEY,
  kind          TEXT NOT NULL,
  name          TEXT NOT NULL,
  display_name  TEXT,
  group_name    TEXT,
  source        TEXT NOT NULL,
  labels_json   TEXT,
  first_seen    TEXT NOT NULL,
  last_seen     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS plugins (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  version       TEXT NOT NULL,
  enabled       INTEGER NOT NULL DEFAULT 0,
  state         TEXT NOT NULL,
  manifest_json TEXT NOT NULL,
  installed_at  TEXT NOT NULL,
  enabled_at    TEXT
);

CREATE TABLE IF NOT EXISTS plugin_permissions (
  plugin_id   TEXT NOT NULL REFERENCES plugins(id),
  capability  TEXT NOT NULL,
  value_json  TEXT NOT NULL,
  granted_by  TEXT NOT NULL,
  granted_at  TEXT NOT NULL,
  PRIMARY KEY (plugin_id, capability, value_json)
);

CREATE TABLE IF NOT EXISTS plugin_settings (
  plugin_id  TEXT NOT NULL REFERENCES plugins(id),
  key        TEXT NOT NULL,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (plugin_id, key)
);

CREATE TABLE IF NOT EXISTS plugin_settings_draft (
  plugin_id  TEXT NOT NULL REFERENCES plugins(id),
  key        TEXT NOT NULL,
  value_json TEXT NOT NULL,
  updated_by TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (plugin_id, key)
);

CREATE TABLE IF NOT EXISTS jobs (
  id          TEXT PRIMARY KEY,
  kind        TEXT NOT NULL,
  plugin_id   TEXT,
  status      TEXT NOT NULL,
  params_json TEXT NOT NULL,
  created_by  TEXT NOT NULL,
  created_at  TEXT NOT NULL,
  started_at  TEXT,
  finished_at TEXT,
  error       TEXT
);
CREATE INDEX IF NOT EXISTS idx_jobs_created_at ON jobs(created_at);

CREATE TABLE IF NOT EXISTS job_events (
  job_id   TEXT NOT NULL REFERENCES jobs(id),
  seq      INTEGER NOT NULL,
  ts       TEXT NOT NULL,
  level    TEXT NOT NULL,
  message  TEXT NOT NULL,
  data_json TEXT,
  PRIMARY KEY (job_id, seq)
);

CREATE TABLE IF NOT EXISTS app_settings (
  key        TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);
