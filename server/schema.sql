-- One row per anonymous test result. No IP address or identifier is stored:
-- the operator, country and city come from Cloudflare's network data.
CREATE TABLE IF NOT EXISTS results (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  country TEXT NOT NULL,
  asn INTEGER NOT NULL,
  org TEXT NOT NULL,
  city TEXT,
  source TEXT NOT NULL,
  down_mbps REAL NOT NULL,
  up_mbps REAL NOT NULL,
  idle_ms REAL,
  loaded_ms REAL
);
CREATE INDEX IF NOT EXISTS results_country_ts ON results (country, ts);

-- Precomputed ranking per country, refreshed by the hourly cron.
CREATE TABLE IF NOT EXISTS ranking (
  country TEXT PRIMARY KEY,
  updated INTEGER NOT NULL,
  json TEXT NOT NULL
);
