-- Service principal: first-class automation identity (FOUND S 1.3.1)
CREATE TABLE service_principals (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    automation_id   UUID,
    display_name    TEXT NOT NULL,
    identity_type   TEXT NOT NULL DEFAULT 'automation',
    governance_profile TEXT NOT NULL DEFAULT 'tool',
    permissions     JSONB NOT NULL DEFAULT '{}',
    credential_id   UUID,
    owner_id        UUID REFERENCES users(id),
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_sp_tenant ON service_principals(tenant_id);
CREATE INDEX idx_sp_automation ON service_principals(automation_id);
CREATE INDEX idx_sp_owner ON service_principals(owner_id);
CREATE INDEX idx_sp_identity_type ON service_principals(identity_type);
