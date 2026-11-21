-- Audit trail: written whenever a student or visit record is viewed, created, edited or deleted.
CREATE TABLE activity_log (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    school_id   uuid NOT NULL REFERENCES schools (id) ON DELETE CASCADE,
    user_id     uuid NOT NULL,
    action      text NOT NULL CHECK (action IN ('view', 'create', 'update', 'delete')),
    table_name  text NOT NULL,
    record_id   uuid NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (user_id, school_id) REFERENCES users (id, school_id)
);

CREATE INDEX activity_log_school_created_idx ON activity_log (school_id, created_at DESC);
CREATE INDEX activity_log_record_idx ON activity_log (table_name, record_id);
