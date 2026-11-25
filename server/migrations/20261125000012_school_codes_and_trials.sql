-- Clinilog is now multi-school (SaaS): the superadmin creates schools, and each
-- school's staff connect with a short school code typed once in the desktop app.

-- School code: 3–20 upper-case letters, digits or hyphens, unique, e.g. VNHS.
-- Existing schools get a random code; the first school keeps a readable one.
ALTER TABLE schools
    ADD COLUMN code text NOT NULL DEFAULT ('S' || upper(substr(md5(random()::text), 1, 7)))
        CHECK (code ~ '^[A-Z0-9-]{3,20}$');

UPDATE schools SET code = 'VNHS' WHERE name = 'Valencia National High School';

CREATE UNIQUE INDEX schools_code_key ON schools (code);

-- New schools start on a free 30-day Basic trial.
ALTER TABLE subscriptions ADD COLUMN is_trial boolean NOT NULL DEFAULT false;
