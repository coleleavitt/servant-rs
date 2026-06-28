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

CREATE TABLE IF NOT EXISTS account_note (
  id         TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  body       TEXT NOT NULL,
  at         TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_account_household ON account(household_id);
CREATE INDEX IF NOT EXISTS idx_holding_account ON holding(account_id);
CREATE INDEX IF NOT EXISTS idx_txn_account ON transaction_ledger(account_id);
CREATE INDEX IF NOT EXISTS idx_order_account ON paper_order(account_id);
CREATE INDEX IF NOT EXISTS idx_note_account ON account_note(account_id);

-- ── Phase 9: the agentic operating model (CONCEPT.md) ───────────────────────
-- A STRATEGY is a first-class trading system: a versioned policy (triggers + rules) that
-- references models/security-sets and is assignable to a managed allocation (CONCEPT.md §4).
CREATE TABLE IF NOT EXISTS strategy (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  kind        TEXT NOT NULL DEFAULT 'rotation',  -- rotation, distress-avoid, follow-disclosures, tactical-derisk
  description TEXT NOT NULL DEFAULT '',
  model_id    TEXT REFERENCES model(id) ON DELETE SET NULL,
  -- rules/triggers as JSON (e.g. {"sellWhenZBelow":1.8,"maxNames":12})
  rules       TEXT NOT NULL DEFAULT '{}',
  version     INTEGER NOT NULL DEFAULT 1,
  created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- A MANAGED ALLOCATION is a slice of cash an agent runs as a local paper portfolio, with a
-- mandate + a HARD per-allocation risk envelope (CONCEPT.md §1). Its own holdings live in the
-- holding/transaction tables keyed by a dedicated paper account_id, so the existing paper engine
-- + deterministic risk gate apply unchanged.
CREATE TABLE IF NOT EXISTS managed_allocation (
  id              TEXT PRIMARY KEY,
  household_id    TEXT NOT NULL REFERENCES household(id) ON DELETE CASCADE,
  source_account_id TEXT REFERENCES account(id) ON DELETE SET NULL, -- where the cash came from (on paper)
  paper_account_id  TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE, -- the allocation's own book
  strategy_id     TEXT REFERENCES strategy(id) ON DELETE SET NULL,
  name            TEXT NOT NULL,
  mandate         TEXT NOT NULL DEFAULT '',          -- free-text objective/constraints
  risk_level      INTEGER NOT NULL DEFAULT 3,        -- 1..5
  funded          REAL NOT NULL DEFAULT 0,           -- cash originally allocated
  -- the hard envelope (mirrors paper::RiskLimits): the agent cannot widen these.
  max_order_frac  REAL NOT NULL DEFAULT 0.25,
  max_cash_use_frac REAL NOT NULL DEFAULT 1.0,
  allow_short     INTEGER NOT NULL DEFAULT 0,
  active          INTEGER NOT NULL DEFAULT 1,        -- kill switch: 0 halts all autonomous activity
  created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- The DECISION LOG: every agent action records the thesis + confidence that justified it, plus
-- the gate verdict (CONCEPT.md §8 audit rail). Append-only.
CREATE TABLE IF NOT EXISTS agent_decision (
  id            TEXT PRIMARY KEY,
  allocation_id TEXT NOT NULL REFERENCES managed_allocation(id) ON DELETE CASCADE,
  step          TEXT NOT NULL,            -- watch, understand, decide, gate, act
  ticker        TEXT NOT NULL DEFAULT '',
  action        TEXT NOT NULL DEFAULT '', -- BUY/SELL/HOLD
  shares        REAL NOT NULL DEFAULT 0,
  thesis        TEXT NOT NULL DEFAULT '',
  confidence    REAL NOT NULL DEFAULT 0,
  admitted      INTEGER NOT NULL DEFAULT 0,  -- gate verdict
  verdict       TEXT NOT NULL DEFAULT '',    -- gate rationale
  at            TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_alloc_household ON managed_allocation(household_id);
CREATE INDEX IF NOT EXISTS idx_decision_alloc ON agent_decision(allocation_id);
