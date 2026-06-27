-- Ponoma DB schema — Orion-parity book of record (SQLite dev / Postgres-ready).
-- Spine: household -> client -> registration -> account -> holding/transaction.
-- Plus models, paper-trade orders/fills, blocks/allocations, billing, audit.
-- All ids are TEXT uuids; money/shares as REAL (analysis-grade; paper trading).

CREATE TABLE IF NOT EXISTS household (
  id           TEXT PRIMARY KEY,
  name         TEXT NOT NULL,
  advisor_rep  TEXT NOT NULL DEFAULT '',
  created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS client (
  id           TEXT PRIMARY KEY,
  household_id TEXT NOT NULL REFERENCES household(id) ON DELETE CASCADE,
  name         TEXT NOT NULL,
  email        TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS registration (
  id         TEXT PRIMARY KEY,
  client_id  TEXT NOT NULL REFERENCES client(id) ON DELETE CASCADE,
  reg_type   TEXT NOT NULL            -- Individual, Joint, IRA, ...
);

CREATE TABLE IF NOT EXISTS custodian (
  id   TEXT PRIMARY KEY,
  name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS model (
  id   TEXT PRIMARY KEY,
  name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS model_holding (
  model_id      TEXT NOT NULL REFERENCES model(id) ON DELETE CASCADE,
  ticker        TEXT NOT NULL,
  target_weight REAL NOT NULL,          -- percent
  PRIMARY KEY (model_id, ticker)
);

CREATE TABLE IF NOT EXISTS account (
  id              TEXT PRIMARY KEY,
  household_id    TEXT NOT NULL REFERENCES household(id) ON DELETE CASCADE,
  registration_id TEXT REFERENCES registration(id) ON DELETE SET NULL,
  custodian_id    TEXT REFERENCES custodian(id) ON DELETE SET NULL,
  model_id        TEXT REFERENCES model(id) ON DELETE SET NULL,
  number          TEXT NOT NULL,
  account_type    TEXT NOT NULL,        -- Individual, Roth IRA, Joint JTWROS, TOD, Trust, 529, HSA, ...
  cash            REAL NOT NULL DEFAULT 0,
  created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS security (
  ticker      TEXT PRIMARY KEY,
  name        TEXT NOT NULL DEFAULT '',
  asset_class TEXT NOT NULL DEFAULT 'equity'
);

CREATE TABLE IF NOT EXISTS holding (
  id         TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  ticker     TEXT NOT NULL,
  shares     REAL NOT NULL,
  cost_basis REAL NOT NULL DEFAULT 0    -- per share
);

CREATE TABLE IF NOT EXISTS transaction_ledger (
  id         TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL,             -- BUY, SELL, DIVIDEND, FEE, CASH
  ticker     TEXT NOT NULL DEFAULT '',
  shares     REAL NOT NULL DEFAULT 0,
  price      REAL NOT NULL DEFAULT 0,
  amount     REAL NOT NULL DEFAULT 0,   -- signed cash impact
  at         TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS paper_order (
  id         TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  action     TEXT NOT NULL,             -- BUY / SELL
  ticker     TEXT NOT NULL,
  shares     REAL NOT NULL,
  status     TEXT NOT NULL DEFAULT 'DRAFT', -- DRAFT/PENDING/BLOCKED/ALLOCATED/PAPER_FILLED/CLOSED/DELETED
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS paper_fill (
  id       TEXT PRIMARY KEY,
  order_id TEXT NOT NULL REFERENCES paper_order(id) ON DELETE CASCADE,
  shares   REAL NOT NULL,
  price    REAL NOT NULL,
  at       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS trade_block (
  id     TEXT PRIMARY KEY,
  ticker TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'OPEN'
);

CREATE TABLE IF NOT EXISTS block_allocation (
  id         TEXT PRIMARY KEY,
  block_id   TEXT NOT NULL REFERENCES trade_block(id) ON DELETE CASCADE,
  account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  shares     REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS fee_schedule (
  id      TEXT PRIMARY KEY,
  name    TEXT NOT NULL,
  basis   TEXT NOT NULL DEFAULT 'AUM',  -- AUM, flat
  -- tiers stored as JSON: [{"upTo": 1000000, "ratePct": 1.0}, ...]
  tiers   TEXT NOT NULL DEFAULT '[]',
  minimum REAL NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS audit_event (
  id           TEXT PRIMARY KEY,
  household_id TEXT REFERENCES household(id) ON DELETE SET NULL,
  action       TEXT NOT NULL,
  entity       TEXT NOT NULL,
  detail       TEXT NOT NULL DEFAULT '',
  at           TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_account_household ON account(household_id);
CREATE INDEX IF NOT EXISTS idx_holding_account ON holding(account_id);
CREATE INDEX IF NOT EXISTS idx_txn_account ON transaction_ledger(account_id);
CREATE INDEX IF NOT EXISTS idx_order_account ON paper_order(account_id);
