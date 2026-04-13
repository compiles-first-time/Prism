-- Tenant model: legal-entity-scoped isolation (FOUND S 1.2)
CREATE TABLE tenants (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL,
    legal_entity_type TEXT NOT NULL,
    parent_tenant_id UUID REFERENCES tenants(id),
    compliance_profiles TEXT[] NOT NULL DEFAULT '{}',
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_tenants_parent ON tenants(parent_tenant_id);
CREATE INDEX idx_tenants_legal_entity_type ON tenants(legal_entity_type);
