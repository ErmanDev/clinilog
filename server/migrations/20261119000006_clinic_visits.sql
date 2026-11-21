CREATE TABLE clinic_visits (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    school_id   uuid NOT NULL REFERENCES schools (id) ON DELETE CASCADE,
    student_id  uuid NOT NULL,
    user_id     uuid NOT NULL,  -- attending staff; replaces the logbook signature
    visited_at  timestamptz NOT NULL,
    complaint   text NOT NULL CHECK (char_length(complaint) BETWEEN 3 AND 500),
    treatment   text NOT NULL CHECK (char_length(treatment) BETWEEN 1 AND 500),
    created_at  timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (student_id, school_id) REFERENCES students (id, school_id),
    FOREIGN KEY (user_id, school_id) REFERENCES users (id, school_id)
);

CREATE INDEX clinic_visits_school_visited_idx ON clinic_visits (school_id, visited_at DESC);
CREATE INDEX clinic_visits_student_idx ON clinic_visits (student_id, visited_at DESC);
