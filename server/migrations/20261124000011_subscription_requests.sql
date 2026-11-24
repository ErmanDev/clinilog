-- Subscription payments are manual: a school admin pays by GCash (InstaPay QR),
-- then submits the reference number and a receipt screenshot. Nothing changes
-- until the superadmin checks the payment and approves (or rejects) the request.
CREATE TABLE subscription_requests (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    school_id       uuid NOT NULL REFERENCES schools (id) ON DELETE CASCADE,
    requested_by    uuid NOT NULL,
    plan            text NOT NULL CHECK (plan IN ('basic', 'premium')),
    term            text NOT NULL CHECK (term IN ('monthly', 'yearly')),
    -- Price at the time of the request, in pesos.
    amount_php      integer NOT NULL CHECK (amount_php > 0),
    payment_method  text NOT NULL DEFAULT 'gcash' CHECK (payment_method = 'gcash'),
    -- Normalised: letters and digits only, upper-case.
    reference_no    text NOT NULL CHECK (reference_no ~ '^[A-Z0-9]{6,30}$'),
    receipt_image   bytea NOT NULL,
    receipt_mime    text NOT NULL CHECK (receipt_mime IN ('image/jpeg', 'image/png', 'image/webp')),
    status          text NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'approved', 'rejected', 'cancelled')),
    created_at      timestamptz NOT NULL DEFAULT now(),
    reviewed_at     timestamptz,
    review_note     text CHECK (review_note IS NULL OR char_length(review_note) <= 500),
    -- The subscription created when the request was approved.
    subscription_id uuid REFERENCES subscriptions (id),
    FOREIGN KEY (requested_by, school_id) REFERENCES users (id, school_id)
);

-- A school has at most one request waiting for review.
CREATE UNIQUE INDEX subscription_requests_one_pending
    ON subscription_requests (school_id) WHERE status = 'pending';

-- A payment reference can only be used once (a rejected or cancelled request frees it).
CREATE UNIQUE INDEX subscription_requests_reference_key
    ON subscription_requests (reference_no) WHERE status IN ('pending', 'approved');

CREATE INDEX subscription_requests_status_created_idx ON subscription_requests (status, created_at DESC);
CREATE INDEX subscription_requests_school_idx ON subscription_requests (school_id, created_at DESC);
