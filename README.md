# Clinilog

A desktop clinic records app for school clinics. The full brief is in `Clinilog – Build Spec.md`.

| Folder | What it is |
| --- | --- |
| `server/` | Axum API (Rust) + PostgreSQL migrations in `server/migrations/` |
| `desktop/` | Tauri 2 app (React + TypeScript + Tailwind) |

These are two independent projects. Each one has its own toolchain and lockfile.

## Server: local setup

Requirements: Rust (stable) and PostgreSQL 13 or newer.

1. Create a database and a role, for example:
   ```sql
   CREATE ROLE clinilog LOGIN PASSWORD 'choose-a-password';
   CREATE DATABASE clinilog OWNER clinilog;
   ```
2. Copy `server/.env.example` to `server/.env` and fill in `DATABASE_URL`, `JWT_SECRET` (at least 32 random characters) and `SEED_ADMIN_PASSWORD`.
3. Seed the database. This also applies the migrations:
   ```sh
   cd server
   cargo run --bin seed
   ```
   The seed creates Valencia National High School, a 12-month Basic subscription, one admin user and 12 sample sections. Running it again does nothing.
4. Start the API:
   ```sh
   cargo run
   ```
   Then open `GET http://localhost:8080/health`. It should return `{"status":"ok","database":"ok"}`.

When the server starts, it applies any pending migrations by itself. You don't need `sqlx-cli`.

## API so far

| Method | Path | Access | Purpose |
| --- | --- | --- | --- |
| GET | `/health` | public | Server and database check |
| POST | `/auth/login` | public | Body `{ school_id, username, password }`. Returns a Bearer token. `school_id` may be left out only for the superadmin |
| GET | `/schools/lookup?code=` | public | Find a school by its code (e.g. `VNHS`). The app calls it once and remembers the school |
| GET | `/auth/me` | signed in | The current user and school |
| GET | `/subscription` | signed in | Plan, expiry, read-only flag, limits and Premium features |
| GET | `/sections` | signed in | Sections, with student counts |
| POST, PUT, DELETE | `/sections`, `/sections/{id}` | admin | Add, rename or delete a section (only if it has no students). Optional client `id` on add makes retries safe |
| GET | `/students?q=&section_id=&grade_level=&page=&page_size=` | signed in | Search by name or LRN prefix, filter by section, paged. `grade_level` needs Premium |
| POST | `/students` | signed in | Add a student. Optional client `id` makes retries safe |
| GET | `/students/lrn-check?lrn=&exclude=` | signed in | Whether an LRN is free in the school, or who has it. `exclude` is the student being edited |
| GET | `/students/{id}` | signed in | Profile plus full visit history. Logged as a view |
| PUT | `/students/{id}` | signed in | Edit a student |
| DELETE | `/students/{id}` | admin | Delete a student who has no visits |
| GET | `/medicines` | signed in | Medicine list, with how many visits used each one |
| POST, PUT, DELETE | `/medicines`, `/medicines/{id}` | admin | Add, edit or delete a medicine (only if no visit used it). Optional client `id` on add makes retries safe |
| GET | `/visits?…` | signed in | Visits, newest first. All plans: `date` (one Philippine day), `student_id`, `q` (student name or LRN), `section_id`. Premium only: `grade_level`, `date_from`, `date_to`, `complaint` |
| POST | `/visits` | signed in | Record a visit. The server sets the time and attending staff. Optional client `id` makes retries safe |
| GET | `/visits/{id}` | signed in | One visit. Logged as a view |
| PUT | `/visits/{id}` | recorder or admin | Edit complaint, treatment and medicines |
| DELETE | `/visits/{id}` | admin | Delete a visit |
| GET | `/dashboard` | signed in | Today's visit count and the 10 most recent visits |
| GET | `/reports/basic?year=&month=` | signed in | Visits and students seen, visits per month for the year, most common complaints |
| GET | `/reports/detailed?year=&month=` | signed in, Premium | Visits by grade and section, medicine usage, frequent visitors |
| GET | `/sync?students=&visits=&deleted=&limit=` | signed in | The school's records for the desktop app's offline copy: students, visits and deletions changed after each cursor (paged), plus all sections, medicines and staff names |
| POST | `/activity/views` | signed in | Student and visit records opened while the app was offline, logged at the time they were viewed. Each has a client `id`, so retries don't log twice |

LRN rules:
- An LRN is exactly 12 ASCII digits after trimming spaces at the ends. Leading zeros are kept.
- Spaces, dashes or other characters inside are rejected, and so are digits from other scripts (for example １２３ or ١٢٣).
- It must be unique per school. The same LRN can exist in two different schools.
- The rules are checked in four places:
  - in the form as you type;
  - by a live "already registered" check against the server;
  - again on save;
  - by the database, with a `CHECK` on the format and `UNIQUE (school_id, lrn)`.
- Simultaneous saves of the same LRN are serialized per school, so exactly one succeeds. The others get a normal LRN error.

Rules for student records:
- Every field is checked on the server against §7a of the spec. A 400 response lists a message for each bad field.
- A student with the same first name, last name and birthdate as an existing one gets a `409 possible_duplicate` warning. Resending with `allow_duplicate: true` saves anyway. Edits only warn again if the name or birthdate changed.
- Basic plans stop at 500 students. Expired subscriptions can view but not change records.
- Viewing, creating, editing and deleting a student are all written to `activity_log`.

Search rules:
- Name search matches the words in order, either as "first last" or "last, first". A single run of digits also matches the start of an LRN.
- A complaint search needs every word to appear, in any order and any case.
- `%` and `_` in search text are matched literally, not as wildcards.
- Date ranges include both end days and use Philippine calendar days.
- On a Basic plan, any Premium-only filter gets a `403` with `code: "premium_required"`.

Subscriptions (paid by GCash, approved by the superadmin):
- **Prices.** Constants in `server/src/subscription.rs` (`price_php`): Basic ₱150/month or ₱1,300/year, Premium ₱300/month or ₱3,600/year. The app shows whatever the server sends.
- **Paying.** The admin scans the InstaPay QR (`server/assets/gcash-qr.png`, served by `GET /subscription/plans`) with GCash and pays. Then they send `POST /subscription/requests` with `{ plan, term, reference_no, receipt_image }`. Both the reference number and the receipt screenshot are required.
- **Nothing changes yet.** The request waits as `pending`. A school can have only one pending request, and the admin can cancel it with `POST /subscription/requests/{id}/cancel`. `GET /subscription/requests` lists the school's history.
- **Checks.** The reference number is stored as letters and digits only, in upper case, and must be 6–30 characters. A reference on a pending or approved request can't be used again. The receipt must really be a PNG, JPG or WEBP (checked from its bytes) and at most 3 MB. The app shrinks photos before sending.
- **Review.** The superadmin compares the receipt and reference with the GCash payment received, then calls `POST /superadmin/requests/{id}/approve` or `/reject`. A rejection needs a reason, and the school sees it.
- **Start dates (decided at approval):**
  - Renewing the same plan while it's active extends it from the day after it ends.
  - Switching plans starts today and cancels the old plan.
  - After expiry, the plan starts today.
- **Expired plans.** Schools can send payments while read-only, because that's when they renew.
- **Activity log.** Requests and cancellations are logged with table `subscription_requests`.

Schools (multi-school / SaaS):
- Each school has a unique **school code**: 3–20 upper-case letters, digits or hyphens, e.g. `VNHS`. The first school (seeded) is `VNHS`.
- The superadmin creates a school with `POST /superadmin/schools` and `{ name, code, address, full_name, username, password }`. That also creates its first admin and copies in the usual medicines. Sections and staff are added by the school's admin.
- **Free trial.** A new school starts on **Basic for 30 days** (today plus 29 days), stored as a subscription with `is_trial = true`. `GET /subscription` reports `is_trial`, and the app shows a "Free trial: N days left" banner.
- **When the trial ends** without payment, the school is read-only like any expired plan, and the admin can still pay.
  - Paying for Basic during the trial starts the day after the trial ends.
  - Paying for Premium starts on approval and ends the trial.
- Usernames only need to be unique **within** a school, so every school can have its own `admin`.

Superadmin:
- One Clinilog-wide account, set on the server with `SUPERADMIN_USERNAME` (default `superadmin`) and `SUPERADMIN_PASSWORD`. The password needs 12+ characters with a letter and a number, or the server won't start. If `SUPERADMIN_PASSWORD` is unset, the superadmin login is off.
- They log in from any copy of the desktop app, with or without a school set (**Clinilog superadmin? Log in here** on the school-code screen). The app then shows only **Payment Requests** and **Schools**.
- Five wrong passwords lock the superadmin login for 5 minutes. The lock is kept in memory, so restarting the server clears it.
- The account can't open any school's records: `require_auth` rejects its token. Its routes are under `/superadmin/*`.
- Changing the password signs the superadmin out everywhere. Schools can't create a user with the superadmin's username.

Account rules:
- Only active accounts count toward the plan limit (Basic 2, Premium 10). Disabling an account frees a slot.
- An admin can't disable or demote themselves, and the last active admin can't be disabled or demoted.
- While the subscription is inactive, admins can still reset passwords and disable accounts. Adding accounts, re-enabling them and changing roles are blocked.
- Disabling an account or changing its role takes effect on that account's next request.
- A password reset doesn't end sessions that are already logged in. Tokens expire after `JWT_TTL_MINUTES`. To cut off access at once, disable the account.
- Account changes are written to the activity log with table `users`.

Report rules:
- A report covers a Philippine calendar year, or one month of it. "Visits per month" always shows the whole year.
- Most common complaints:
  - Complaints are free text, so they're counted by the words used. Each word counts once per visit, and filler words in English and Filipino are ignored.
  - A two-word phrase seen in 2 or more visits, such as "stomach ache", replaces its words when it accounts for every mention of them.
  - Only words or phrases used in 2 or more visits are listed.
- Print and Save as PDF use the same print layout. Save as PDF shows the Windows Save dialog and writes an A4 PDF through WebView2's PrintToPdf. Windows only.

Rules for clinic visits:
- The attending staff member is always the logged-in user.
- The visit time is the server's current time. A visit recorded offline may send the time it was recorded instead, which can't be in the future (5 minutes of clock drift is allowed) or more than 30 days old.
- Complaint must be 3–500 characters and treatment 1–500. Each medicine quantity must be a whole number from 1 to 100, with up to 20 medicines per visit and no medicine listed twice.
- Viewing, creating, editing and deleting a visit are all written to `activity_log`.
| GET, POST | `/users` | admin | List accounts in the admin's school, or add one |
| PUT | `/users/{id}` | admin | Change full name and role |
| POST | `/users/{id}/disable`, `/users/{id}/enable` | admin | Disable or re-enable an account |
| POST | `/users/{id}/reset-password` | admin | Set a new password. Also clears a login lock |
| GET | `/activity?user_id=&action=&table=&date_from=&date_to=&page=` | admin | Activity log, newest first |
| GET | `/subscription/plans` | admin | Prices, limits, features and the GCash QR |
| GET, POST | `/subscription/requests` | admin | The school's payment history, or send a payment (reference number + receipt) |
| POST | `/subscription/requests/{id}/cancel` | admin | Withdraw a payment that's still waiting for review |
| GET | `/superadmin/requests?status=` | superadmin | Payment requests (`pending` by default, or `approved`, `rejected`, `cancelled`, `all`) |
| GET | `/superadmin/requests/{id}` | superadmin | One request with its receipt image and the school's current plan |
| POST | `/superadmin/requests/{id}/approve`, `/reject` | superadmin | Approve (starts the plan) or reject (`{ note }` required) |
| GET, POST | `/superadmin/schools` | superadmin | Every school with its plan, trial, expiry, usage and pending payments; or create a school with its first admin |

Sign-in rules:
- Usernames are case-insensitive. Passwords are case-sensitive.
- Five failed attempts lock that username for 5 minutes.
- Tokens last `JWT_TTL_MINUTES` (default 8 hours).
- Disabling an account or changing its role takes effect on the account's next request.
- Every signed-in query is filtered by the user's own `school_id`.

## Deployment

The API runs on Render from `render.yaml`. That Blueprint creates a Docker web service and a PostgreSQL database, both in the Singapore region.

1. In Render, choose **New → Blueprint**, connect GitHub, and pick this repo.
2. When asked, enter **`SEED_ADMIN_PASSWORD`**. This becomes the first admin's password: at least 8 characters, with a letter and a number.
3. Click **Apply**. The first Docker build takes about 5–10 minutes.
4. Open the **clinilog-api** service, then **Logs**, and find the `seeded first school … school_id=…` line. The first school's code is `VNHS`.
5. Check `https://<service>.onrender.com/health`. It should return `{"status":"ok","database":"ok"}`.
6. Build the desktop app with that URL (see [Building the installer](#building-the-installer)). Add more schools as the superadmin under **Schools → Add school**.

Notes:
- `JWT_SECRET` is generated by Render, and `DATABASE_URL` is linked to the database automatically. Neither is ever in the code.
- The database has no outside access. Only the API reaches it, over Render's private network.
- Migrations run automatically when the service starts.
- Seeding only happens once. Later starts skip it, so `SEED_ADMIN_PASSWORD` can be deleted afterwards.
- Set **`SUPERADMIN_PASSWORD`** (Environment tab) for the account that approves payments. A Blueprint only asks for `sync: false` values when it's first created, so on an existing service add it by hand.
- The database uses Render's smallest paid plan, `basic-256mb`. Free databases are deleted after 30 days, and a workspace can only have one.
- The web service uses the **free** plan, which sleeps when idle. The first request after that takes up to a minute; the app shows "Connecting to server…" meanwhile. For everyday clinic use, switch it to `starter` or higher.

## Desktop app

Requirements: Node 20 or newer, Rust (stable, MSVC on Windows) and WebView2 (included with Windows 11).

Settings:

| Setting | Build-time env var | Notes |
| --- | --- | --- |
| Server URL | `CLINILOG_API_URL` | Release builds require `https://`. Debug builds default to `http://localhost:8080` |
| School | `CLINILOG_SCHOOL_ID` (optional) | **Leave it out** for one installer that works for every school: on first launch the app asks for the school code. Set it only to lock an installer to one school |

When someone enters a school code, the app saves the school in `config.json` in its config folder: `%APPDATA%\com.clinilog.desktop\config.json` on Windows. **Change school** on the login screen forgets it. That's refused while visits recorded on the PC still need to upload. The file can also set the server URL without a rebuild:

```json
{ "api_url": "https://clinilog-api.onrender.com", "school_id": "…", "school_name": "…", "school_code": "VNHS" }
```

Run in development (with the server running):

```sh
cd desktop
npm install
npm run tauri dev
```

To build the Windows installers, see [Building the installer](#building-the-installer) below.

The window opens fullscreen with no Windows titlebar. Clinilog's own Minimize and Close buttons are at the top right.

### Offline mode

The app keeps working without the internet. When the connection is back, it uploads what was done offline and downloads what changed, by itself.

- **What works offline.** Dashboard, students (search, view, add, edit, delete), visits (search, view, record, edit, delete), the Search page, basic and detailed reports (including Print and Save as PDF), and sections and medicines in Settings. Pages are answered from the records saved on the PC, with the same checks, messages and permission rules as the server (`desktop/src-tauri/src/local.rs`). User accounts, the activity log and the subscription page show the last copy seen on this PC, and changing them needs the internet.
- **The offline copy.** After an online login, the app downloads the school's sections, students, visits, medicines and staff names (`GET /sync`), then only what changed, every 30 seconds. Deleted records are removed too. The sidebar shows when the records were last updated.
- **Saving changes.** Online, changes go straight to the server. If the server can't be reached, the change is checked on the PC, saved in an encrypted outbox, and shown right away. Changes upload in the order they were made, one person at a time. A background check runs every 30 seconds (every 10 seconds while offline), and there is also a **Sync now** button. New records get an ID made on the PC, so a retried upload never creates a duplicate. Visits keep the time they were recorded.
- **Two PCs changing the same record.** The upload that reaches the server last wins. Things the server checks again on upload, such as an LRN another PC took meanwhile, are refused and shown under "Waiting to upload".
- **Changes the server refuses.** For example, the student was deleted, or the LRN is now taken. These show under "Waiting to upload" with the reason, and a **Try again** or **Discard** button. For a student, **Try again** also means "save anyway" after a same-name-and-birthdate warning.
- **Audit trail.** Students and visits opened offline are logged and uploaded with the time they were viewed (`POST /activity/views`).
- **What's stored.** The school's records above, saved copies of the server-only pages (per user, so a staff account never sees an admin's), records opened offline, unsent changes, and a password check value for offline login (an Argon2 hash, never the password).
- **Encryption.** The cache is a local SQLite file at `%APPDATA%\com.clinilog.desktop\offline-cache.sqlite3`. Every row is encrypted with AES-256-GCM and tied to its table and row. The 256-bit key is protected by Windows DPAPI, so a copied file can't be read on another PC or Windows account. This was chosen instead of SQLCipher, which would need Strawberry Perl on every build machine.
- **Offline login.** Works for an account that logged in on this PC while online and hasn't logged out since. 5 wrong passwords lock it for 5 minutes. When the connection returns, the app logs back in by itself. If the password was changed or the account disabled in the meantime, it asks the person to log in again, and their waiting changes are kept.
- **Plan.** Offline, the last known plan applies for 7 days. After that, records are view-only until the app reconnects. If the subscription expires before offline changes upload, they wait and upload once it's renewed.
- **Logout.** Clears the offline copy, saved pages and saved logins. Changes that haven't uploaded are kept, encrypted, and upload the next time the person who made them logs in online. They stay with that person so they're credited correctly.

### Building the installer

Requirements:
- Node 20 or newer.
- Rust (stable, MSVC toolchain), which needs Visual Studio Build Tools with the "Desktop development with C++" workload.
- WebView2 (included with Windows 11).
- No other tools. SQLite is compiled from source by the C compiler above.

```sh
cd desktop
npm install
CLINILOG_API_URL=https://<your-service>.onrender.com npm run tauri build
```

This one installer works for every school; each school types its code once. Add `CLINILOG_SCHOOL_ID=<school uuid>` only to make an installer locked to one school.

The installers are written to `desktop/src-tauri/target/release/bundle/` (`msi/` and `nsis/`). The first build downloads the WiX and NSIS tools.

The URL (and the school ID, if set) are built into the app, so always rebuild after changing them. Old files in `bundle/` are only replaced when a build succeeds.

If the `.msi` step fails with `light.exe : error LGHT0001 : JIT Compiler encountered an internal limitation`, run the same command again. This is an occasional crash in the .NET runtime WiX uses, not a problem in the code. To build only the `-setup.exe`, add `-- --bundles nsis`.

How the app handles sign-in and the plan:
- All server calls go through the Rust side of the app. The login token stays in memory there and never reaches the web page.
- After login, the app fetches the school's plan, and refreshes it every 30 minutes. It saves the last known plan locally.
- If the server can't be reached, the app uses the last known plan for up to 7 days, then switches to read-only.
- Premium-only items stay visible but locked. The server enforces the real limits.
