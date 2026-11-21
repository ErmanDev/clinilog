CREATE TABLE sections (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    school_id     uuid NOT NULL REFERENCES schools (id) ON DELETE CASCADE,
    grade_level   integer NOT NULL CHECK (grade_level BETWEEN 1 AND 12),
    section_name  text NOT NULL CHECK (char_length(btrim(section_name)) BETWEEN 1 AND 50),
    UNIQUE (school_id, grade_level, section_name),
    UNIQUE (id, school_id)
);
