-- Failed-login tracking for the lockout rule: 5 failed attempts lock that
-- username for 5 minutes. One row per (school, lower-cased username); the row
-- is deleted on a successful login. Usernames that don't exist are tracked too,
-- so a locked response never reveals whether an account exists.
CREATE TABLE login_attempts (
    school_id       uuid NOT NULL REFERENCES schools (id) ON DELETE CASCADE,
    username        text NOT NULL CHECK (username = lower(username)),
    failed_count    integer NOT NULL DEFAULT 0 CHECK (failed_count >= 0),
    last_failed_at  timestamptz NOT NULL DEFAULT now(),
    locked_until    timestamptz,
    PRIMARY KEY (school_id, username)
);
