CREATE TABLE students (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    school_id    uuid NOT NULL REFERENCES schools (id) ON DELETE CASCADE,
    section_id   uuid NOT NULL,
    lrn          char(12) NOT NULL CHECK (lrn ~ '^[0-9]{12}$'),
    first_name   text NOT NULL CHECK (char_length(first_name) BETWEEN 1 AND 50),
    middle_name  text CHECK (middle_name IS NULL OR char_length(middle_name) BETWEEN 1 AND 50),
    last_name    text NOT NULL CHECK (char_length(last_name) BETWEEN 1 AND 50),
    gender       text NOT NULL CHECK (gender IN ('male', 'female')),
    birthdate    date NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (school_id, lrn),
    UNIQUE (id, school_id),
    FOREIGN KEY (section_id, school_id) REFERENCES sections (id, school_id)
);

CREATE INDEX students_school_name_idx ON students (school_id, last_name, first_name);
CREATE INDEX students_section_idx ON students (section_id);

-- Keep updated_at current on every UPDATE.
CREATE FUNCTION set_updated_at() RETURNS trigger AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER students_set_updated_at
    BEFORE UPDATE ON students
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
