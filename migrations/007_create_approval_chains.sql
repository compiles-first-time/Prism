-- Approval chains computed by LCA algorithm (FOUND S 1.4.1)
CREATE TABLE approval_chains (
    id              UUID PRIMARY KEY,
    automation_id   UUID NOT NULL REFERENCES automations(id),
    scope           TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    requested_by    UUID NOT NULL REFERENCES users(id),
    approvers       JSONB NOT NULL DEFAULT '[]',
    conditions      JSONB,
    decided_at      TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_approval_chains_automation ON approval_chains(automation_id);
CREATE INDEX idx_approval_chains_status ON approval_chains(status);
CREATE INDEX idx_approval_chains_requested_by ON approval_chains(requested_by);
