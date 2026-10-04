# DCA Bot Suite

DCA Bot Suite is the Discord bot **and** its web dashboard for the Discord Drivers community, in one Rust process that shares one bot token and one state store:

- `bot/native/` - the Rust workspace: the Discord bot (slash and `-` commands, recruitment tickets, reaction roles, team counts, YouTube checks, team-event spreadsheets and reports), the dashboard API with Discord sign-in, and the **local PaddleOCR** screenshot reader.
- `dashboard/` - the React dashboard used to configure servers, channels, roles, tickets, member counts, feeds, spreadsheets and bot logging. It is built to static files and served by the bot process.

Production state lives in Firebase (the same documents the earlier Node bot used, so existing data keeps working). Without Firebase, local JSON files are used for development.

> Moving from the Node bot? Read [docs/MIGRATION.md](docs/MIGRATION.md): what stayed identical, what changed on purpose, and the known limits.

## Repository Layout

```text
.
├── bot/
│   ├── native/            Rust workspace
│   │   ├── dca-bot/       Discord bot, managers, commands, dashboard API and OAuth
│   │   ├── dca-core/      PaddleOCR vision, standings and licence reading, XLSX/report/image output
│   │   └── dca-state/     Firebase (Firestore / Realtime Database) + local JSON state, typed config
│   ├── assets/mastery/    Images for /sportscar and /garage
│   ├── fonts/             Bundled Noto fonts (text + colour emoji) for rendered images and emoji matching
│   ├── fixtures/guides/   The two guide screenshots (also served by the dashboard)
│   ├── scripts/download-models.sh
│   └── models/            PaddleOCR ONNX models (downloaded, not committed)
├── dashboard/             React app (Vite): src/, public/
├── deploy/aws/            EC2 provisioning, update script, service units, Caddy config
└── docs/                  Migration notes, performance figures, live-test checklist, team-event guide
```

## Requirements

- A Discord application with a bot user and a bot token with the **Server Members** and **Message Content** privileged intents enabled (the bot requests Guilds, Guild Messages, Guild Members, Reactions, Moderation, Message Content and Direct Messages).
- A Firebase project with Cloud Firestore or Realtime Database for production state (optional for development).
- To run from source: Rust (stable, 1.80+) and a C++ toolchain (ONNX Runtime is linked statically), plus Node 18+ only to build the dashboard.

Screenshots (driver licences, team-event standings) are read by **local PaddleOCR (PP-OCRv5 on ONNX Runtime)**: no API keys, no paid services, no outside calls and no quota. See [Screenshot reading](#screenshot-reading-local-paddleocr).

## Run it

```bash
# 1. dashboard (once, and after dashboard changes)
cd dashboard && npm install && npm run build && cd ..

# 2. OCR models (~100 MB, once; the bot also downloads missing models by itself)
sh bot/scripts/download-models.sh

# 3. the bot + dashboard (slash commands are registered on start)
cd bot/native
DISCORD_TOKEN=... cargo run --release -p dca-bot
```

Everything is read from the environment (a `.env` file in the repository root, `bot/` or the working directory is loaded). Open `http://localhost:3000/dashboard` for the dashboard.

### Environment

Required:

```env
DISCORD_TOKEN=your_bot_token
FIREBASE_PROJECT_ID=your_firebase_project_id
FIREBASE_SERVICE_ACCOUNT=service_account_json_or_base64_json
FIREBASE_DATABASE_URL=https://your-project-id-default-rtdb.firebaseio.com
```

The bot exits at start when `DISCORD_TOKEN` is missing (same as before). Slash commands are registered globally every time the bot starts, so `npm run deploy:commands` no longer exists. Set `DCA_SKIP_COMMAND_DEPLOY=1` to skip it.

Recommended:

```env
DISCORD_GUILD_ID=fallback_server_id
COMMUNITY_GUILD_ID=community_server_id
RECRUITMENT_GUILD_ID=recruitment_server_id
RECRUITER_ROLE_ID=role_that_can_manage_recruitment
FIREBASE_DATABASE_TYPE=realtime
FIREBASE_STATE_ROOT=dca_bot_state
PORT=3000
```

Dashboard sign-in:

```env
DISCORD_CLIENT_ID=your_application_id
DISCORD_CLIENT_SECRET=your_oauth_secret
DASHBOARD_BASE_URL=https://your-dashboard.example.com
DISCORD_REDIRECT_URI=https://your-dashboard.example.com/auth/discord/callback
DASHBOARD_SESSION_SECRET=a_long_random_secret
DASHBOARD_ALLOWED_ROLE_ID=role_that_can_open_dashboard
```

Optional:

```env
DASHBOARD_UPLOAD_CHANNEL_ID=discord_channel_for_tutorial_uploads
DASHBOARD_UPLOAD_LIMIT=100mb
DASHBOARD_UPLOAD_DIR=/path/for/uploads        # when no upload channel is set
DASHBOARD_ROLE_RECHECK_MINUTES=5
DASHBOARD_ROLE_RECHECK_GRACE_MINUTES=30
DASHBOARD_SESSION_HOURS=8
SPREADSHEET_IMAGE_RETENTION_DAYS=7
RECRUITMENT_SCREENSHOT_DM_USER_ID=user_that_stores_applicant_screenshots
DCA_DATA_DIR=bot/data                         # local JSON state and generated spreadsheets
DCA_MODELS_DIR=bot/models  DCA_FONTS_DIR=bot/fonts  DCA_ASSETS_DIR=bot/assets  DASHBOARD_DIST_DIR=dashboard/dist
DCA_MODEL_BASE_URL=...                        # mirror for the OCR model files
DCA_OCR_DEBUG=1                               # print name candidates while reading
RUST_LOG=info
DCA_OCR_THREADS=1                             # ONNX threads (default: what the container may use)
DCA_OCR_QUICK_SECS=20  DCA_OCR_CHECK_SECS=45  # how long an applicant's screenshot check may take before it is accepted on trust
DCA_OCR_RECYCLE_MB=205                        # rebuild the OCR sessions above this resident size (default 40% of the memory limit)
```

The bot exposes `GET /` and `GET /health` (200 only while connected to Discord, 503 otherwise, plus the OCR state: `ready`, `loading` or an error) and serves the dashboard and its API on the same port.

### Dashboard sign-in

The redirect URI in the Discord Developer Portal must exactly match `DISCORD_REDIRECT_URI`, for example `https://your-dashboard.example.com/auth/discord/callback`. If login fails, check `DISCORD_CLIENT_ID`, `DISCORD_CLIENT_SECRET`, `DASHBOARD_BASE_URL`, `DISCORD_REDIRECT_URI`, the role id, that the bot is in the configured server, and that cookies are allowed for the dashboard domain. Sessions are signed cookies; the allowed role is re-checked every few minutes with a grace period if Discord is briefly unreachable.

## Server Model

- Community server - welcome, leave, reaction roles, YouTube posts, member count, dashboard access role, and destination invites.
- Recruitment server - recruitment panel, ticket threads, recruiter roles, ban list, screenshot guide uploads, and recruitment logs.

The dashboard Spreadsheet page can choose monitored channels, output channels and access roles from both servers.

## Recruitment Tickets

The recruitment system posts an Apply button. Applicants upload their driver's licence (and optionally team-event screenshots) in the panel channel; the bot mirrors them to a private DM store, checks that they are real in-game screenshots (one quick pass, nothing else is read), asks for corrections with the guide images when a screenshot is wrong, and opens a private thread with **Claim** and **Close** buttons.

```text
/tickets setup | sync-panel | sync-banlist | status | logs
/tickets claim | close | add | massadd | remove | rename | archive | delete
/tickets screenshot-list | screenshot-add | screenshot-change | screenshot-remove
/invite
/ban
```

Ticket close outcomes are configured in the dashboard. Accepted recruits trigger member-count updates and delayed role assignment in the recruitment and community servers. Closing a ticket sends a recruitment log embed at once (Discord user ID, team joined, screenshot counts, licence screenshot), locks and archives the thread, and stores the transcript. For an accepted recruit the screenshots are then read once in the background (one text pass per image) and the embed is completed with the in-game name, previous team, garage power and the applicant's team-event rank, points and score. That is the only OCR a recruitment needs; ticket creation just checks that the screenshots are real game screenshots.

## Screenshot reading (local PaddleOCR)

One reader (`dca-core`) serves recruitment and the spreadsheets:

- **Engine**: PP-OCRv5 text detection plus per-script recognisers (multilingual, Latin, Cyrillic, Arabic, Korean, Thai, Greek, Devanagari, Tamil, Telugu) on ONNX Runtime, in-process.
- **Layout independent**: it does not assume where anything is. Standings rows come from the score column, ranks and names by position, and the row team from the colour of the band (yellow = own team, blue = opponent). Licences are read by their label words. Ranks are repaired from neighbouring rows and anything uncertain is flagged instead of guessed.
- **Any capture**: any resolution or aspect ratio, cropped images (tight crops of only the table), letterboxed or stretched screenshots, sideways and upside-down files, and **photos of a device** (phone, tablet, a monitor with the game on it). A fallback ladder tries the image as is, autocropped, with the standings table flattened, with the screen rectified, and in all four orientations, and keeps the best reading.
- **Names**: every name is read by several recognisers and voted, and emoji are matched against the bundled Noto Color Emoji set, so names like `📦DC|BlackWing` survive. Flags and rank badges are not read as part of the name.
- **Instant check for applicants**: a screenshot is recognised from its text labels in a moment, so applicants are let through at once and the ticket is created automatically (also when a licence and event screenshots arrive in the same upload). Only photos and unusual files need the full reader, within a deadline.
- **Fails open**: if the models are missing, still loading, or a check runs out of time, applicants are not blocked (the ticket is flagged for recruiters) and spreadsheet sessions report what failed.
- **Small hosts**: one inference thread, sessions recycled when memory grows, language models loaded on demand. Measured on a 0.1 CPU / 512 MB container in [docs/PERFORMANCE.md](docs/PERFORMANCE.md).

Honest limits: Hebrew (no PaddleOCR model), emoji in a style other than Noto's, tiny superscript digits (`ARSH⁴⁴⁴` reads `ARSH44`) and white-heavy emoji can be misread; heavily blurred or extremely angled photos may only be recognised, not read. Treat the output as a head start and use the correction commands. Send real samples that fail to `bot/fixtures/samples/` to extend the test set.

Models load in the background after start (and are downloaded once if missing), so the bot is up immediately; `/health` shows the OCR state.

## Team Counts And Roles

Member count teams are configured in the dashboard Members page: display name, division, player count, recruitment status, recruitment-server and community-server roles, auto-assignment delay and aliases (used to match what is read from screenshots).

```text
/membercount set | sync | list
/teamcount
/updatecount
```

## Team Event Spreadsheet System

The bot watches configured team channels for image attachments. Screenshots from the same user in the same channel are grouped into one pending session during the grouping window (default 1 minute). After the window the session is read locally and the bot posts only generated outputs: the event XLSX (Summary, Ranking, Attendance and chart sheets), a spreadsheet preview image and a summary chart image. Parsing normalises the readings, applies staff corrections and computes statistics.

Dashboard Spreadsheet team settings: enabled, monitored channel, output channel, team access role, own-team aliases, known own players, auto process.

Global settings: grouping window, output format (`xlsx` or `fods`), raw data retention days (stored page readings, default 31), local image retention days (leftover generated images, default 7), LibreOffice path (leave blank; XLSX is written directly).

Screenshot rules: send all screenshots of one event together; several images in one message or several messages from the same user are appended while the window is open; podium, summary, standings and cropped list screenshots can be mixed; do not submit two events in one window.

### Spreadsheet commands

```text
/spreadsheets status | sessions | generate [session_id] [rerun_ocr] | summary [session_id]
/spreadsheets weekly | monthly [anchor_date]
/spreadsheets correct | correct-name | correct-team | correct-placement | correct-points | correct-event-name
/spreadsheets rebuild | regenerate-weekly | regenerate-monthly | file | chart
```

`anchor_date` is `YYYY-MM-DD`; without it reports use the date of the latest processed session. Temporary development commands, marked `TEMP` in Discord: `test-ocr`, `test-grouping`, `preview`, `rebuild-event`, `force-weekly`, `force-monthly`.

### Automatic weekly and monthly reports

Normal event output posts only the XLSX, the preview image and the chart. A weekly report is posted when the parsed event name changes from the previous processed event; a monthly report is posted by the scheduler during the first three UTC days of the new month, once per team and output channel. Rules: every processed session is one event; missing players score `0`; the period maximum is the sum of each event's maximum; `%kill` is `total / max`; `#KAB` counts events where the player ranked above every opponent (`0` when no opponent rows exist). Report workbooks have a `Report` sheet and a `Details` sheet.

### Correction workflow

If a name, team, rank, points, score or event name is read wrong, use the `correct-*` commands. Corrections are an override layer on the session; rebuilds replay the stored page readings plus corrections, so nothing is read twice, and later reports use the corrected data. Stored readings are cleaned after the raw data retention period.

## Other commands

Slash: `/help`, `/ping`, `/dashboard`, `/invite`, `/whois`, `/yt`, `/ban`, `/warn`, `/clearwarns`, `/snap`, `/roles`, `/remindme`, `/reminders`, `/cancelreminder`, `/top3te`, `/top3km`, `/teameventsummary`, `/sportscar`, `/garage`, `/membercount`, `/teamcount`, `/updatecount`, `/tickets`, `/spreadsheets`.

Prefix (`-`): `announce`, `bam`, `ban`, `clean`, `clearwarns`, `help`, `kick`, `ping`, `team`, `mkick`, `mute`, `pingmessage`, `snapban`, `temperature` (`temp`, `weather`), `unban`, `warn`, `warnings`, `whois`, `yt`. `-help` and `/help` list everything.

## Dashboard Pages

Overview, Tickets (active tickets, logs, transcripts, tutorials, settings), Reaction Roles, YouTube, Spreadsheets (team channels, output channels, roles, report setup), Members, Logs, Server (server ids, dashboard role, recruiter role, manager role, command log channel, dashboard URL).

## State Storage

With Firebase configured, state is stored there. Realtime Database is recommended (spreadsheet state can grow large); Cloud Firestore is also supported.

```env
FIREBASE_DATABASE_TYPE=realtime            # or firestore
FIREBASE_DATABASE_URL=https://your-project-id-default-rtdb.firebaseio.com
FIREBASE_STATE_ROOT=dca_bot_state          # Firestore: FIREBASE_STATE_COLLECTION=dca_bot_state
```

Each scope is stored as `{ "data": ..., "updatedAt": ... }` under the root or collection: `dashboardConfig`, `recruitmentTickets`, `recruitmentLogs`, `recruitmentBans`, `botLogs`, `warnings`, `spreadsheetSessions`, `spreadsheetReportEmissions`, `teamRoleAssignments` and (new) `reminders`. Credentials: `FIREBASE_SERVICE_ACCOUNT` (raw or base64 JSON), `FIREBASE_SERVICE_ACCOUNT_PATH` or `GOOGLE_APPLICATION_CREDENTIALS`. The service-account JSON can be shared with any previous deployment: nothing needs migrating.

Without Firebase, scopes are JSON files in `bot/data/` (override with `DCA_DATA_DIR`) and generated spreadsheets are under `bot/data/spreadsheets/`. A state document that fails to load is never overwritten by an empty one.

## Deployment

Production runs on a single AWS EC2 instance (about $15/month): see [deploy/aws/README.md](deploy/aws/README.md). Pushing to `main` deploys by itself: GitHub Actions tests it, builds the bot and the dashboard once the tests pass, and the server installs the new build within a few minutes (and goes back to the previous one if it does not start properly). `deploy/aws/provision.sh` creates the instance. The same release build runs on any Ubuntu 24.04 host with `deploy/aws/bootstrap.sh` and `update.sh`. Use the same Firebase project and credentials as before. Register `https://<your-host>/auth/discord/callback` as the OAuth redirect.

## Tests

```bash
cd bot/native && cargo test --workspace
```

The suite includes a fake Discord server (`dca-bot/src/mock_discord.rs`) that the bot's real code talks to, so whole flows run without a connection: applying with a licence, auto ticket creation, closing a ticket, the five-screenshot / 96-player sample event (`bot/fixtures/samples`, ground truth in `team-event-expected.tsv`), spreadsheet commands, reaction roles, welcome/leave, YouTube, panels and the commands. `bot/native/.cargo/config.toml` raises the test stack size.

Covers the state layer (including legacy Node records and concurrent writes), report maths and spreadsheet generation, the dashboard HTTP API, and end-to-end runs of the recruitment reading and the spreadsheet pipeline on the guide screenshots (these use the OCR models and skip themselves when they are not downloaded). No Discord connection or API keys are needed.

## Troubleshooting

- Slash commands missing: restart the bot (they are registered on start) and check the log for `Registered N global command(s)`.
- Poor spreadsheet reads: use uncropped, high-resolution screenshots or a straight photo of the whole table; add own team aliases; use `/spreadsheets preview` or `test-ocr`; fix rows with `/spreadsheets correct-*` and `rebuild`.
- `/health` shows `ocr: loading` or an error: the models are being downloaded or cannot be fetched; run `sh bot/scripts/download-models.sh` or set `DCA_MODEL_BASE_URL`.
- Reports empty: confirm processed sessions exist in the period, use `anchor_date`, and check the team id matches the dashboard team.
- Reports not posted: check the output channel, the bot's send/attach permissions and that auto process is on.

## Contributors

- Drago
- Devil
- BlackWing
