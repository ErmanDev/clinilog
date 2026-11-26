-- Offline copy for the desktop app: lets a PC ask "what changed since my last sync?"
-- (see src/sync.rs).

-- Visits can be edited (complaint, treatment, medicines), so they need updated_at like students.
-- Editing a visit's medicines always updates its clinic_visits row too, so this covers them.
ALTER TABLE clinic_visits ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now();

CREATE TRIGGER clinic_visits_set_updated_at
    BEFORE UPDATE ON clinic_visits
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX students_sync_idx ON students (school_id, updated_at, id);
CREATE INDEX clinic_visits_sync_idx ON clinic_visits (school_id, updated_at, id);

-- Students and visits that were deleted, so PCs holding an offline copy drop them too.
-- No foreign key to schools: rows are written while a school's records are being deleted.
CREATE TABLE sync_deletions (
    school_id   uuid NOT NULL,
    table_name  text NOT NULL CHECK (table_name IN ('students', 'clinic_visits')),
    record_id   uuid NOT NULL,
    deleted_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX sync_deletions_idx ON sync_deletions (school_id, deleted_at, record_id);

CREATE FUNCTION record_sync_deletion() RETURNS trigger AS $$
BEGIN
    INSERT INTO sync_deletions (school_id, table_name, record_id)
    VALUES (OLD.school_id, TG_TABLE_NAME, OLD.id);
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER students_record_deletion
    AFTER DELETE ON students
    FOR EACH ROW EXECUTE FUNCTION record_sync_deletion();

CREATE TRIGGER clinic_visits_record_deletion
    AFTER DELETE ON clinic_visits
    FOR EACH ROW EXECUTE FUNCTION record_sync_deletion();
