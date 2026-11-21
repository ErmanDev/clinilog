-- Schools (tenants). gen_random_uuid() is built into PostgreSQL 13+.
-- Clients may also supply their own uuid (needed for offline-created records).
CREATE TABLE schools (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text NOT NULL CHECK (char_length(btrim(name)) BETWEEN 1 AND 200),
    address     text NOT NULL DEFAULT '',
    created_at  timestamptz NOT NULL DEFAULT now()
);
