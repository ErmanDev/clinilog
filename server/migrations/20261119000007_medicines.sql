CREATE TABLE medicines (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    school_id  uuid NOT NULL REFERENCES schools (id) ON DELETE CASCADE,
    name       text NOT NULL CHECK (char_length(btrim(name)) BETWEEN 1 AND 100),
    unit       text NOT NULL CHECK (char_length(btrim(unit)) BETWEEN 1 AND 30),
    UNIQUE (school_id, name)
);

CREATE TABLE visit_medicines (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    visit_id     uuid NOT NULL REFERENCES clinic_visits (id) ON DELETE CASCADE,
    medicine_id  uuid NOT NULL REFERENCES medicines (id),
    quantity     integer NOT NULL CHECK (quantity BETWEEN 1 AND 100),
    UNIQUE (visit_id, medicine_id)
);

CREATE INDEX visit_medicines_medicine_idx ON visit_medicines (medicine_id);
