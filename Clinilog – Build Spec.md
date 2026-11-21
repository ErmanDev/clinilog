# Clinilog – Build Spec

Use this file as the project brief for Cursor. Build the project step by step in the order given in "Build order". Ask before changing the stack or the schema.

## 1. Overview

The app is named Clinilog. Use this name in the window title, installer, login screen and package names. Clinilog is a desktop app for school clinics, first for Valencia National High School (VNHS), Valencia City, Bukidnon. Clinic staff record student health profiles and clinic visits, search records and print reports. Schools pay a subscription (Basic or Premium). The app is online: data lives on our server, with a local cache so visits can still be logged during internet outages.

Students never use the app. They tell the nurse their complaint and the nurse types it in.

## 2. Tech stack

- Desktop app: Tauri 2 (Rust backend, TypeScript + React frontend, Tailwind CSS)
- Local cache: SQLite (via `rusqlite` or `sqlx`) for the offline queue and cached data
- Server API: Rust with Axum
- Server database: PostgreSQL, migrations with `sqlx`
- Auth: password hashing with `argon2`; JWT access tokens
- LRN entered manually (no camera or OCR)
- All traffic over HTTPS

Repository layout:

- `/desktop` – Tauri app
- `/server` – Axum API
- `/server/migrations` – SQL migrations

## 3. Roles and sidebar

Two roles: `admin` and `staff`. Role comes from the logged-in user, not the computer. The same app runs on any clinic PC or laptop.

Admin sidebar: Dashboard, Students, Clinic Visits, Search, Reports, User Accounts, Subscription, Activity Log, Settings.

Staff sidebar: Dashboard, Students, Clinic Visits, Search, Reports.

## 4. Plans and feature gating

| Feature | Basic | Premium |
| --- | --- | --- |
| Student records | up to 500 | unlimited |
| User accounts | 2 | up to 10 |
| Search | name, section | advanced filters (date range, complaint, grade) |
| Reports | basic | detailed |
| Backups (server side) | weekly | daily |

Rules:

- Premium-only items stay visible but locked, with a lock icon. Clicking shows an "Upgrade to Premium" message.
- Every limit and Premium check is also enforced on the server. The UI lock is only for display.
- On launch, the app asks the server for the school's plan and expiry date.
- If the subscription expires, switch to read-only. Never block viewing of existing records.
- If the server can't be reached, allow a 7-day grace period using the last known plan.

## 5. Database schema (PostgreSQL, 3NF)

Every school-owned table carries `school_id` so one server can serve many schools.

- `schools`: id (uuid, PK), name, address, created\_at
- `subscriptions`: id (uuid, PK), school\_id (FK), plan (`basic` | `premium`), start\_date, end\_date, status
- `users`: id (uuid, PK), school\_id (FK), full\_name, username (unique per school), password\_hash, role (`admin` | `staff`), is\_active, created\_at
- `sections`: id (uuid, PK), school\_id (FK), grade\_level (int), section\_name; unique (school\_id, grade\_level, section\_name)
- `students`: id (uuid, PK), school\_id (FK), section\_id (FK), lrn (12 digits, unique per school), first\_name, middle\_name (nullable), last\_name, gender, birthdate, created\_at, updated\_at
- `clinic_visits`: id (uuid, PK), school\_id (FK), student\_id (FK), user\_id (FK, the staff member who attended; replaces the logbook signature), visited\_at (timestamp), complaint (text), treatment (text), created\_at
- `medicines`: id (uuid, PK), school\_id (FK), name, unit
- `visit_medicines`: id (uuid, PK), visit\_id (FK), medicine\_id (FK), quantity
- `activity_log`: id (uuid, PK), school\_id (FK), user\_id (FK), action, table\_name, record\_id, created\_at

Notes:

- Do not store age. Calculate it from birthdate.
- Write to `activity_log` whenever a student or visit record is viewed, created, edited or deleted.

## 6. Screens

- Login: username, password.
- Dashboard: today's visit count, recent visits list.
- Students: list with search, add/edit form (LRN, first, middle, last name, gender, birthdate, section). Student detail page shows full visit history.
- Clinic Visits: new-visit form (pick student, complaint, treatment, optional medicines with quantity). Date/time and attending staff fill in automatically.
- Search: basic for all, advanced filters for Premium.
- Reports: visits per month, most common complaints; print and PDF export.
- Admin only: User Accounts (add, disable, reset password, enforce plan limit), Subscription (plan, expiry), Activity Log, Settings.

## 7. LRN entry

- Staff type the LRN manually on the student form. No camera or barcode scanning.
- Accept only exactly 12 digits; show an inline error otherwise.
- Reject an LRN already registered in the same school.

## 7a. Input validation

Validate every field twice: in the form (instant inline error under the field, save button disabled until valid) and again on the server (reject with a clear message). Trim spaces before checking.

| Field | Rule |
| --- | --- |
| First / last name | Required, 1–50 chars, letters, spaces, hyphen, apostrophe, period; allow ñ and accents |
| Middle name | Optional, same rule as names |
| LRN | Required, exactly 12 digits, unique per school |
| Gender | Required, pick from a dropdown only |
| Birthdate | Required, valid date, not in the future, age 4–25 |
| Section | Required, picked from existing sections |
| Visit date/time | Auto-filled, not in the future |
| Complaint | Required, 3–500 chars |
| Treatment | Required, 1–500 chars |
| Medicine quantity | Whole number, 1–100 |
| Username | Required, 4–30 chars, letters, digits, underscore, unique per school |
| Password | At least 8 chars, with a letter and a number |

Also:

- Warn (not block) when a student with the same full name and birthdate already exists.
- Use parameterized SQL queries only, never string-built SQL.
- Escape all text shown in the UI.
- Lock login for 5 minutes after 5 failed attempts.
- Ask for confirmation before deleting any record.

## 8. Offline cache and sync

- New visits save to local SQLite first, marked `pending`, then upload when online.
- Each record uses a uuid created on the client, so uploads are safe to retry.
- Show an online/offline indicator and the number of pending items.
- Cache only what the clinic needs; clear the cache on logout.

## 9. Security and privacy

Student health data is sensitive personal information under the Philippine Data Privacy Act (RA 10173).

- HTTPS only; hashed passwords; tokens expire.
- Every API query is filtered by the logged-in user's `school_id`.
- Role checks on the server for admin-only endpoints.
- Encrypt the local SQLite cache (SQLCipher).
- Daily or weekly database backups depending on plan.

## 9a. Deployment

Only the backend is deployed online. The desktop app is built once and installed on each clinic PC.

- Backend: deploy the Axum server to Render as a web service, with a Render PostgreSQL database. Keep secrets (database URL, JWT secret) in Render environment variables, never in code.
- Desktop app: build the Tauri app into a Windows installer (`.msi` / `.exe`) and share it for download. No hosting needed for the app itself.
- The app reads the backend's Render URL from a config value set at build time, so it can be changed without code edits.
- Render's free tier sleeps when idle, so the first request can take a while. Show a "Connecting to server…" state, and use a paid instance for real clinic use.

## 10. Build order

1. Set up the repo, the Axum server, the Postgres migrations for all tables, and seed data (one school, one admin, sample sections).
2. Auth: login endpoint, JWT, role middleware, school scoping.
3. Tauri app shell: login screen, role-based sidebar, plan fetch and locked Premium items.
4. Students and sections CRUD.
5. Clinic visits and medicines.
6. Search (basic, then advanced).
7. Reports with print/PDF.
8. Admin: user accounts with plan limits, subscription view, activity log.
9. LRN validation (12 digits, unique per school).
10. Offline cache and sync.
11. Tests for the API, plus a manual test checklist for user acceptance testing with the clinic.
