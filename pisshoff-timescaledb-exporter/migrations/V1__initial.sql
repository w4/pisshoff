CREATE TABLE audit (
    timestamp TIMESTAMPTZ NOT NULL,
    connection_id UUID NOT NULL,
    peer_address TEXT NOT NULL,
    host TEXT NOT NULL,
    UNIQUE(timestamp)
);

SELECT create_hypertable('audit', 'timestamp');

CREATE TABLE audit_environment_variables (
    connection_id uuid NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL
);

CREATE INDEX audit_environment_variables_connection_id ON audit_environment_variables USING HASH (connection_id);
CREATE INDEX audit_environment_variables_name ON audit_environment_variables USING HASH (name);

CREATE TABLE audit_events (
    timestamp TIMESTAMPTZ NOT NULL,
    connection_id UUID NOT NULL,
    type TEXT NOT NULL,
    content JSONB
);

SELECT create_hypertable('audit_events', 'timestamp');

CREATE INDEX audit_events_connection_id ON audit_events USING HASH (connection_id);
CREATE INDEX audit_events_type ON audit_events USING HASH (type);