CREATE TABLE subscriptions (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    school_id   uuid NOT NULL REFERENCES schools (id) ON DELETE CASCADE,
    plan        text NOT NULL CHECK (plan IN ('basic', 'premium')),
    start_date  date NOT NULL,
    end_date    date NOT NULL,
    status      text NOT NULL CHECK (status IN ('active', 'expired', 'cancelled')),
    CHECK (end_date >= start_date)
);

CREATE INDEX subscriptions_school_end_idx ON subscriptions (school_id, end_date DESC);
