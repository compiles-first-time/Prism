-- Visibility compartments and membership (SR_GOV_31, SR_GOV_32, SR_GOV_33)
-- Compartments isolate data by classification level with explicit membership.
-- Criminal-penalty compartments override "visibility flows up" (GAP-77).

CREATE TABLE IF NOT EXISTS compartments (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    name            TEXT NOT NULL,
    classification_level TEXT NOT NULL,
    purpose         TEXT NOT NULL,
    criminal_penalty_isolation BOOLEAN NOT NULL DEFAULT FALSE,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (tenant_id, name)
);

CREATE INDEX idx_compartments_tenant ON compartments(tenant_id);
CREATE INDEX idx_compartments_classification ON compartments(tenant_id, classification_level);

CREATE TABLE IF NOT EXISTS compartment_members (
    compartment_id  UUID NOT NULL REFERENCES compartments(id),
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    person_id       UUID,
    role_id         UUID,
    added_at        TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- At least one of person_id or role_id must be set
    CHECK (person_id IS NOT NULL OR role_id IS NOT NULL),
    -- Prevent duplicate membership entries
    UNIQUE (compartment_id, tenant_id, person_id, role_id)
);

CREATE INDEX idx_compartment_members_person ON compartment_members(tenant_id, person_id)
    WHERE person_id IS NOT NULL;
CREATE INDEX idx_compartment_members_role ON compartment_members(tenant_id, role_id)
    WHERE role_id IS NOT NULL;
CREATE INDEX idx_compartment_members_compartment ON compartment_members(compartment_id);
