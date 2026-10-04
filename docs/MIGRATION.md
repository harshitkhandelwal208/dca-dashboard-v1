# Migration notes: Node bot to the Rust bot

The Rust bot is a port of the Node bot (discord.js, Express, Firebase, Gemini). Users should not notice the move: same commands and options, same buttons and embeds, same dashboard, same stored data. This page lists what is intentionally different.

## What stayed the same

- Every slash command and `-` command, with the same names, options, permissions and wording (including the hard-coded OpenWeather key behind `-temperature`).
- The recruitment flow end to end: Apply panel, licence upload in the channel, DM mirror, screenshot checks with guide images, optional event screenshots, private ticket thread with Claim and Close, outcome buttons, transcript, log embed, member count update, delayed role assignment, lock and archive.
- Reaction roles, YouTube notifier, welcome and leave messages, member-count message, recruitment ban list panel, team role scheduler, ping-settings message, panel self-heal every minute.
- Team-event spreadsheets: grouping window, session lifecycle, 3-sheet XLSX, preview image, chart image, weekly report on event-name change, monthly report in the first three UTC days, `#KAB`, missed players scored 0, correction commands, retention cleanup.
- The React dashboard and its Discord sign-in, the same API paths, and the same state documents in Firebase (or the same JSON files). Existing production data is read in place.

## Changes on purpose

**OCR (replaces Gemini).**
- Screenshots are read locally with PaddleOCR (PP-OCRv5 on ONNX Runtime) from Rust. No API keys, no network calls, no quota. The Gemini fields (recruitment key, spreadsheet model, timeout, retries, per-team key) are gone from the dashboard and the config; `/spreadsheets generate ... rerun_gemini` is now `rerun_ocr` and `/spreadsheets test-gemini` is `test-ocr`.
- Works on cropped images, any aspect ratio, rotated files and photos of devices, and keeps emoji and many scripts in names (see the README for the list and the known limits).
- Sessions created by the Gemini version have no stored page readings; rebuilding them uses their saved players, and `generate ... rerun_ocr:true` reads the screenshots again.
- Anything the reader is unsure of is flagged rather than guessed; recruitment checks fail open (applicants are never blocked by OCR trouble).

**Recruitment only checks that the screenshots are real (changed).**
- When a ticket is created the bot makes **one** cheap text pass per screenshot and only decides "is this an in-game HCR2 screenshot?" (the profile card, the team-event table or the result screen). Nothing else is read at that point. The stats (in-game name, previous team, garage power, the applicant's team-event rank/points/score) are read **once, in the background, when an accepted ticket is closed**: the "Recruitment Closing Log" is posted immediately (applicant, Discord user ID, team joined, screenshot counts, who closed it, licence image, "Reading the screenshots...") and edited with the stats a moment later, one text pass per image. A recruitment therefore needs OCR exactly once.
- A recognised screenshot goes straight through: on a normal machine the check takes a fraction of a second. Sideways pictures are turned and looked at again; anything that is not the game gets the guide and another try, as before.
- When a licence and team-event screenshots arrive in the same upload, nothing is left to ask: the ticket is created automatically. (With only a licence the bot still asks whether there are event screenshots to add.)
- If the check cannot finish in time (`DCA_OCR_QUICK_SECS`, default 20 s, on a very slow host) the upload is accepted on trust and the ticket carries a "Screenshot check" note for recruiters. The check has its own lane, so background work never queues it.

**One text pass for spreadsheet screenshots (new).** The spreadsheet channels are dedicated to team-event screenshots, so each image is read once (detect + recognise): the line pass supplies the rows, the names it is sure of are kept as read (only doubtful names, names with emoji and glued lines are read again), and the full photo/rotation ladder only runs for an image that did not give a table. Chat messages and anything without an attachment cost nothing, and session lookups touch only the sessions they need.

**Team-event screenshots in any shape (new).** Cropped pages of the standings, overlapping pages (rows repeated at the seams), pages in any order, and the result ("podium") screen are all understood: the result screen supplies the event name, both team scores and the opponent's name and confirms the top names. Rank repair uses the row grid, so a row the detector loses never shifts the ranks.

**One process, one image.** The bot and dashboard share a port and a Docker image (Node is only used to build the dashboard). Models load in the background at start, so a slow download never delays the bot. `/health` and `/` return 503 until Discord is connected and also report the OCR state.

**Slash commands register on start** (global, bulk overwrite) instead of a separate `deploy:commands` step; `DCA_SKIP_COMMAND_DEPLOY=1` opts out.

**Reliability**
- Reminders are persisted (new `reminders` scope) and checked by a polling scheduler, so they survive restarts. They are delivered by DM and fall back to the channel.
- State writes are serialised per scope and skipped when nothing changed; a scope that failed to load is never overwritten with defaults. Legacy records written by Node load unchanged.
- Panels (recruitment, member count, ban list) are only edited when they actually differ, which avoids needless edits and rate limits.
- The YouTube check interval is re-read from the config on every cycle (a dashboard change applies without a restart).
- DM-mirrored screenshots get a fresh signed URL whenever they are needed, so closing an old ticket does not depend on expired links.
- If a private thread cannot be created the bot falls back to a public thread instead of failing the application.
- The members of the recruiter role are added to every new ticket thread (in the background, at most 40), and the recruiter who claims a ticket is added too. Pinging a role notifies people but never put them in the thread. Failures to add anyone are logged instead of ignored.
- Event-name comparison for weekly reports tolerates OCR differences in spacing and case.
- A panic in a background task is isolated (the release build unwinds instead of aborting).

**Fixes of old behaviour**
- The `/garage` menu now shows the sports-car mastery when a vehicle is chosen (in the Node bot choosing did nothing).

## Small hosts

The reader is tuned for Render's free tier (0.1 CPU, 512 MB): one inference thread, the sessions are rebuilt whenever the process has grown past 40% of the container's memory limit, rarely used language models are loaded on demand and at most two stay in memory, and the allocator is limited to one arena. See [PERFORMANCE.md](PERFORMANCE.md) for measurements.

## Known limits

- Hebrew names have no PaddleOCR model and may be misread; emoji drawn in a style very different from Noto Color Emoji, white-heavy emoji, and tiny superscript digits can be misread.
- Blurred or extremely angled photos may be recognised but not fully read; use the correction commands.
- Real-world samples beyond the two guide screenshots could not be collected automatically. Drop failing screenshots into `bot/fixtures/samples/` to extend the tests.

## Files that are now obsolete

Nothing was deleted automatically. These untracked leftovers from earlier attempts are not used by the workspace and can be removed: `bot/native/ocrd/`, `bot/native/dca-state/src/{log_store,recruitment_store,reminder_store,spreadsheet_store,warning_store}.rs`, `bot/native/dca-core/build.rs`, `bot/native/dca-core/src/{excel,geometry,pixels,recruitment_vision,reports,review_image,standings_table,team_event_vision,tesseract}.rs`, `bot/native/dca-bot/src/{handlers,schedulers,server}/`, `bot/native/dca-bot/src/commands/recruitment.rs`, the root `Cargo.lock` and `target/`.
