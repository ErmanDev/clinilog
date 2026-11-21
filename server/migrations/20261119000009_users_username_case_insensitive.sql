-- Usernames are case-insensitive: "Admin" and "admin" are the same account.
-- Passwords stay case-sensitive (argon2 hashes the exact bytes).
-- Login must look users up with lower(username) = lower($1) so this index is used.
ALTER TABLE users DROP CONSTRAINT users_school_id_username_key;
CREATE UNIQUE INDEX users_school_username_lower_key ON users (school_id, lower(username));
