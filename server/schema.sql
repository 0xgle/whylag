-- One row per anonymous test result. No IP address or identifier is stored:
-- the operator and country come from Cloudflare's network data, and the time
-- is rounded to the hour.
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

-- Daily submission counter per connection, so one person cannot flood a
-- provider's numbers. The key is a hash with a random salt that exists for
-- one day only; both are deleted the next day, so keys cannot be traced back.
CREATE TABLE IF NOT EXISTS quota (
  day TEXT NOT NULL,
  key TEXT NOT NULL,
  n INTEGER NOT NULL,
  PRIMARY KEY (day, key)
);
CREATE TABLE IF NOT EXISTS salt (
  day TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
