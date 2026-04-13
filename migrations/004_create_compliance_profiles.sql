-- Compliance profile configuration (GAP-24)
CREATE TABLE compliance_profiles (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    profile_type    TEXT NOT NULL,
    required_reviewers INTEGER NOT NULL DEFAULT 1,
    compartment_id  UUID,
    review_frequency_days INTEGER,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(tenant_id, name)
);

CREATE INDEX idx_compliance_profiles_tenant ON compliance_profiles(tenant_id);
CREATE INDEX idx_compliance_profiles_type ON compliance_profiles(profile_type);
