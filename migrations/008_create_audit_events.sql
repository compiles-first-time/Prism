-- Append-only audit event log with cryptographic hash chain
CREATE TABLE audit_events (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    event_type      TEXT NOT NULL,
    actor_id        UUID NOT NULL,
    actor_type      TEXT NOT NULL,
    target_id       UUID,
    target_type     TEXT,
    governance_authority TEXT,
    payload         JSONB NOT NULL DEFAULT '{}',
    prev_event_hash TEXT,
    event_hash      TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Audit events are append-only: no UPDATE or DELETE should ever be issued.
-- The event_hash chain provides tamper evidence.

CREATE INDEX idx_audit_events_tenant ON audit_events(tenant_id);
CREATE INDEX idx_audit_events_type ON audit_events(event_type);
CREATE INDEX idx_audit_events_actor ON audit_events(actor_id);
CREATE INDEX idx_audit_events_target ON audit_events(target_id);
CREATE INDEX idx_audit_events_created ON audit_events(created_at);
