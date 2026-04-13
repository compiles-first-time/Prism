-- Registered automations governed by PRISM
CREATE TABLE automations (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    service_principal_id UUID REFERENCES service_principals(id),
    name            TEXT NOT NULL,
    description     TEXT,
    lifecycle_state TEXT NOT NULL DEFAULT 'draft',
    compliance_profiles TEXT[] NOT NULL DEFAULT '{}',
    owner_id        UUID NOT NULL REFERENCES users(id),
    platform_type   TEXT,
    external_ref    TEXT,
    blast_radius_tier TEXT NOT NULL DEFAULT 'contained',
    environment     TEXT NOT NULL DEFAULT 'DEV',
    sunset_date     TIMESTAMPTZ,
    next_review_date TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_automations_tenant ON automations(tenant_id);
CREATE INDEX idx_automations_owner ON automations(owner_id);
CREATE INDEX idx_automations_state ON automations(lifecycle_state);
CREATE INDEX idx_automations_sp ON automations(service_principal_id);
CREATE INDEX idx_automations_review ON automations(next_review_date)
    WHERE lifecycle_state = 'active';
