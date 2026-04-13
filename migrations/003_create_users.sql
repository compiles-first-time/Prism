-- Human users synced from IdP (GAP-21)
CREATE TABLE users (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    idp_id          TEXT,
    email           TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    role_ids        UUID[] NOT NULL DEFAULT '{}',
    primary_reporting_line UUID REFERENCES users(id),
    secondary_reporting_line UUID REFERENCES users(id),
    department      TEXT,
    business_unit   TEXT,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(tenant_id, email)
);

CREATE INDEX idx_users_tenant ON users(tenant_id);
CREATE INDEX idx_users_idp_id ON users(idp_id);
CREATE INDEX idx_users_email ON users(email);
