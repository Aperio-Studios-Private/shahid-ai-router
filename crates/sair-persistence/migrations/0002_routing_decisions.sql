-- Phase 3 additions (review findings R3-03/R3-07): routing decision records,
-- health-event correlation, per-provider loopback opt-in.
ALTER TABLE providers ADD COLUMN allow_loopback INTEGER NOT NULL DEFAULT 0;
ALTER TABLE request_records ADD COLUMN decision_json TEXT;
ALTER TABLE health_events ADD COLUMN correlation_id TEXT;
CREATE INDEX idx_health_events_correlation ON health_events (correlation_id);
