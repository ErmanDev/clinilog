CREATE TABLE users (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    school_id      uuid NOT NULL REFERENCES schools (id) ON DELETE CASCADE,
    full_name      text NOT NULL CHECK (char_length(btrim(full_name)) BETWEEN 1 AND 100),
    username       text NOT NULL CHECK (username ~ '^[A-Za-z0-9_]{4,30}$'),
    password_hash  text NOT NULL,
    role           text NOT NULL CHECK (role IN ('admin', 'staff')),
    is_active      boolean NOT NULL DEFAULT true,
    created_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (school_id, username),
    -- Lets child tables use a composite FK so a row can never point at another school's user.
    UNIQUE (id, school_id)
);
