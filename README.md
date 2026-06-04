# DCA Bot Suite

DCA Bot Suite contains two deployable applications that share one Discord bot token and one state store:

- `bot/` - the Discord bot runtime, slash commands, recruitment tickets, reaction roles, team counts, YouTube checks, Gemini spreadsheet generation, and automatic team-event reports.
- `dashboard/` - the React and Express dashboard used to configure servers, channels, roles, tickets, member counts, feeds, spreadsheets, and bot logging.

Both apps can use the same database through `DATABASE_URL`. PostgreSQL URLs and Azure SQL connection strings are supported. If no database URL is provided, local JSON files in `bot/data/` are used for development.

## Repository Layout

```text
.
├── bot/
│   ├── commands/
│   │   ├── slash/
│   │   └── text/
│   ├── events/
│   ├── utils/
│   ├── data/
│   ├── index.js
│   └── deploy-commands.js
├── dashboard/
│   ├── server/
│   ├── src/
│   ├── public/
│   └── server.js
├── logs/
└── README.md
```

## Requirements

- Node.js 18 or newer.
- A Discord application with a bot user.
- A bot token with the needed gateway intents enabled.
- A shared database for production. PostgreSQL and Azure SQL are supported; the Azure deployment uses Azure SQL Database's free offer.

The team-event spreadsheet system uses:

- Google Gemini Flash for image text extraction and structured parsing.
- `sharp` for screenshot normalization before sending images to Gemini.
- `exceljs` for XLSX files.

Gemini uses the REST `generateContent` API with inline image data. The implementation follows Google's official Gemini API shape for `inline_data` multimodal requests: https://ai.google.dev/api

Required recruitment Gemini environment:

```env
RECRUITMENT_GEMINI_API_KEY=google_ai_studio_key_for_recruitment_license_ocr
```

Spreadsheet Gemini keys are configured per team in the dashboard. They intentionally default to blank.

Optional Gemini and XLSX environment:

```env
GEMINI_FLASH_MODEL=gemini-2.5-flash
GEMINI_API_BASE_URL=https://generativelanguage.googleapis.com
GEMINI_TIMEOUT_MS=300000
GEMINI_MAX_RETRIES=4
SPREADSHEET_IMAGE_RETENTION_DAYS=7
LIBREOFFICE_PATH=soffice
```

LibreOffice is only a fallback because XLSX files are written directly by Node.

## Root Scripts

```bash
npm run start:bot
npm run start:dashboard
npm run deploy:commands
```

`start:bot` deploys slash commands first and then runs the bot. For local development where commands are already deployed, use:

```bash
cd bot
npm run start:runtime
```

## Bot Setup

Install and run:

```bash
cd bot
npm install
npm run deploy:commands
npm start
```

Required bot environment:

```env
DISCORD_TOKEN=your_bot_token
DISCORD_CLIENT_ID=your_application_id
DATABASE_URL=database_connection_string_shared_with_dashboard
DATABASE_CLIENT=postgres_or_sqlserver
RECRUITMENT_GEMINI_API_KEY=google_ai_studio_key_for_recruitment_license_ocr
```

Recommended bot environment:

```env
DISCORD_GUILD_ID=fallback_server_id
COMMUNITY_GUILD_ID=community_server_id
RECRUITMENT_GUILD_ID=recruitment_server_id
RECRUITER_ROLE_ID=role_that_can_manage_recruitment
DATABASE_SSL=true
PORT=3001
```

The bot exposes:

- `/` - basic status.
- `/health` - health check endpoint for hosting platforms.

## Dashboard Setup

Install, build, and run:

```bash
cd dashboard
npm install
npm run build
npm start
```

Open:

```text
http://localhost:3000/dashboard
```

Required dashboard environment:

```env
DISCORD_TOKEN=your_bot_token
DISCORD_CLIENT_ID=your_application_id
DISCORD_CLIENT_SECRET=your_oauth_secret
DATABASE_URL=same_database_connection_string_as_bot
DATABASE_CLIENT=postgres_or_sqlserver
DASHBOARD_BASE_URL=https://your-dashboard.example.com
DISCORD_REDIRECT_URI=https://your-dashboard.example.com/auth/discord/callback
DASHBOARD_SESSION_SECRET=a_long_random_secret
```

Bootstrap access environment:

```env
DISCORD_GUILD_ID=fallback_server_id
COMMUNITY_GUILD_ID=community_server_id
RECRUITMENT_GUILD_ID=recruitment_server_id
DASHBOARD_ALLOWED_ROLE_ID=role_that_can_open_dashboard
```

Optional upload environment:

```env
DASHBOARD_UPLOAD_CHANNEL_ID=discord_channel_for_tutorial_uploads
DASHBOARD_UPLOAD_LIMIT=100mb
DASHBOARD_ROLE_RECHECK_GRACE_MINUTES=30
DATABASE_SSL=true
```

On Vercel, prefer a Discord upload channel for tutorial videos because serverless disk storage is temporary.

## Discord OAuth Redirect

The redirect URI in the Discord Developer Portal must exactly match `DISCORD_REDIRECT_URI`.

Example:

```text
https://your-dashboard.example.com/auth/discord/callback
```

If login fails with a callback or OAuth exchange error, check:

- `DISCORD_CLIENT_ID`
- `DISCORD_CLIENT_SECRET`
- `DASHBOARD_BASE_URL`
- `DISCORD_REDIRECT_URI`
- The redirect URI registered in the Discord Developer Portal

## Server Model

The bot supports two server IDs:

- Community server - welcome, leave, reaction roles, YouTube posts, member count, dashboard access role, and destination invites.
- Recruitment server - recruitment panel, ticket threads, recruiter roles, ban list, screenshot guide uploads, and recruitment logs.

The dashboard Spreadsheet page can choose monitored channels, output channels, and access roles from both the community and recruitment servers.

## Recruitment Tickets

The recruitment system posts an Apply button. Applicants answer prompts and upload screenshots. Recruiters manage the ticket using slash commands and buttons.

Main commands:

```text
/tickets setup
/tickets sync-panel
/tickets sync-banlist
/tickets status
/tickets logs
/tickets claim
/tickets close
/tickets add
/tickets massadd
/tickets remove
/tickets rename
/tickets screenshot-list
/tickets screenshot-add
/tickets screenshot-change
/tickets screenshot-remove
/tickets archive
/tickets delete
/invite
/ban
```

Ticket close outcomes are configured in the dashboard. Accepted recruits can trigger member-count updates and delayed role assignment in the recruitment and community servers. Closing a ticket sends a recruitment log embed built from Gemini Flash extraction, including the Discord user ID, previous team, team joined, garage power, named team-event scores, and the driver's license screenshot.

## Team Counts And Roles

Member count teams are configured in the dashboard Members page. Each team can have:

- Display name.
- Division.
- Player count.
- Recruitment status.
- Recruitment server role assignment.
- Community server role assignment after rules are accepted.
- Auto-assignment delay.
- Aliases for matching Gemini extraction and recruitment data.

Useful commands:

```text
/membercount set
/membercount sync
/membercount list
/teamcount
/updatecount
```

## Team Event Spreadsheet System

The spreadsheet system watches configured team channels for image attachments. When screenshots are posted, the bot groups images into a pending team-event session for the configured grouping window. A session can contain one image, multiple images in one message, or multiple messages from the same user/channel during the window.

After the window closes, Gemini Flash receives all images in the session together. The prompt asks Gemini to extract the visible event name, player rows, ranks, event points, total scores, team labels, own-team versus opponent classification, podium/summary data, and raw visible text. The parser then normalizes that JSON, applies staff corrections, calculates statistics, generates the final XLSX plus a spreadsheet preview image, and posts only those generated event outputs.

Dashboard Spreadsheet team settings:

- Enabled - enables capture for that team.
- Monitored channel - screenshot input channel, from either server.
- Output channel - where generated files and reports are posted, from either server.
- Team access role - role allowed to use spreadsheet commands, from either server.
- Own team aliases - names used to identify own-team rows in Gemini output.
- Auto process - automatically process sessions after the grouping window.

Global Spreadsheet settings:

- Grouping window - minutes to group screenshots from the same user, default 1.
- Output format - `xlsx` or `fods`.
- Gemini Flash model - defaults to `gemini-2.5-flash`.
- Gemini timeout ms - defaults to `300000`.
- Gemini retries - defaults to `4`.
- Raw data retention days - how long raw Gemini text/JSON stays in state before cleanup.
- Local image retention days - maximum age for generated spreadsheet/report images that could not be deleted immediately after posting, default 7.
- LibreOffice path - leave blank for direct `exceljs` XLSX writing.

The Gemini API key is configured by environment variable, not in the dashboard:

```env
RECRUITMENT_GEMINI_API_KEY=google_ai_studio_key_for_recruitment_license_ocr
```

Screenshot submission rules:

- Use uncropped screenshots when possible.
- Send all screenshots for one event close together in the monitored channel.
- Multiple images in one Discord message are treated as one submission.
- Multiple messages from the same user in the same channel are appended while the grouping window is open.
- Podium, summary, standings, and cropped list screenshots can be mixed in one session.
- Do not submit screenshots from two different events in the same grouping window unless you want them parsed as one session.

## Spreadsheet Commands

```text
/spreadsheets status team:<team>
/spreadsheets sessions team:<team>
/spreadsheets generate team:<team> [session_id] [rerun_gemini]
/spreadsheets summary team:<team> [session_id]
/spreadsheets weekly team:<team> [anchor_date]
/spreadsheets monthly team:<team> [anchor_date]
/spreadsheets correct team:<team> session_id:<id> row:<rank> field:<field> value:<value>
/spreadsheets correct-name team:<team> session_id:<id> row:<rank> value:<name>
/spreadsheets correct-team team:<team> session_id:<id> row:<rank> team_type:<own|opponent> [value:<label>]
/spreadsheets correct-placement team:<team> session_id:<id> row:<rank> placement:<rank>
/spreadsheets correct-points team:<team> session_id:<id> row:<rank> field:<points|score> value:<number>
/spreadsheets correct-event-name team:<team> session_id:<id> value:<event>
/spreadsheets rebuild team:<team> session_id:<id>
/spreadsheets regenerate-weekly team:<team> [anchor_date]
/spreadsheets regenerate-monthly team:<team> [anchor_date]
/spreadsheets file team:<team> [session_id]
/spreadsheets chart team:<team> [session_id]
```

`anchor_date` uses `YYYY-MM-DD`. If omitted, weekly and monthly reports use the date of the latest processed session.

Temporary development commands:

```text
/spreadsheets test-gemini team:<team> [session_id]
/spreadsheets test-grouping team:<team>
/spreadsheets preview team:<team> [session_id]
/spreadsheets rebuild-event team:<team> session_id:<id>
/spreadsheets force-weekly team:<team> [anchor_date]
/spreadsheets force-monthly team:<team> [anchor_date]
```

These are clearly marked `TEMP` in Discord and can be removed after production confidence is high.

## Automatic Weekly And Monthly Reports

Normal event output posts only:

- The final event XLSX.
- A generated image preview of the spreadsheet data.
- A generated summary chart image.

No normal-event embeds are posted to the output channel. The event XLSX contains summary, ranking, attendance, chart, and raw Gemini sheets.

Weekly reports are generated when the parsed team event name changes from the previous processed event for that team. The weekly report includes the event names covered that week, player totals, attendance, missed events, #KAB totals, and zero-score events.

Monthly reports are generated automatically after month end by the bot scheduler. The scheduler checks during the first three UTC days of the new month and posts the previous calendar month's report once per team/output channel. Staff can force a monthly report with `/spreadsheets regenerate-monthly` or `/spreadsheets force-monthly`.

Report rules:

- Every processed session in the period is treated as one team event.
- Event columns use the Gemini-detected team event name from the screenshot when available.
- Missing players receive a score of `0` for that event.
- The period max score is the sum of each event's max score.
- `%kill` is `total / max`.
- `#KAB` is the number of events where the player ranked above every opponent.
- If no opponent rows were detected for an event, `#KAB` is `0` because opponent order cannot be proven.
- Weekly reports include the team event names in both the spreadsheet columns and the Details sheet.
- Monthly reports use the same scoring rules over the calendar month.

The generated workbooks contain:

- `Report` sheet with rank, name, event scores, `%kill`, total, max, `#KAB`, missed events, and attended events.
- `Details` sheet with period metadata, report rules, event names, dates, session IDs, max scores, and players parsed.

## Gemini Correction Workflow

If Gemini reads a player name, team, rank, points, score, or event name incorrectly:

```text
/spreadsheets correct team:<team> session_id:<id> row:<rank> field:<field> value:<value>
/spreadsheets correct-name team:<team> session_id:<id> row:<rank> value:<name>
/spreadsheets correct-team team:<team> session_id:<id> row:<rank> team_type:<own|opponent> [value:<label>]
/spreadsheets correct-placement team:<team> session_id:<id> row:<rank> placement:<rank>
/spreadsheets correct-points team:<team> session_id:<id> row:<rank> field:<points|score> value:<number>
/spreadsheets correct-event-name team:<team> session_id:<id> value:<event>
```

Corrections are saved as an override layer on the session. Rebuilds always replay Gemini extraction plus staff corrections, so the original raw extraction remains auditable until retention cleanup. Weekly and monthly reports reflect corrected session data the next time they are generated, regenerated, or when another event triggers a report.

## Help Commands

Slash help:

```text
/help
```

Prefix help:

```text
-help
```

The help output covers recruitment, spreadsheets, weekly/monthly reports, member counts, moderation, reminders, feeds, and dashboard access.

## Dashboard Pages

- Overview - bot and server status.
- Tickets - active tickets, logs, transcripts, tutorials, and ticket settings.
- Reaction Roles - dashboard-managed reaction role panels.
- YouTube - RSS feed checks and announcement channels.
- Spreadsheets - Gemini settings, team channels, output channels, roles, and report setup.
- Members - member count and role assignment settings.
- Logs - combined bot and recruitment logs.
- Server - community and recruitment server IDs, dashboard role, recruiter role, manager role, command log channel, and dashboard URL.

## State Storage

With `DATABASE_URL`, state is stored in the configured database table, default:

```env
STATE_TABLE_NAME=dca_bot_state
DATABASE_CLIENT=postgres_or_sqlserver
```

Without `DATABASE_URL`, local JSON fallback files are used:

```text
bot/data/dashboardConfig.json
bot/data/recruitmentTickets.json
bot/data/recruitmentLogs.json
bot/data/recruitmentBans.json
bot/data/botLogs.json
bot/data/spreadsheetSessions.json
```

Generated spreadsheet outputs are written under:

```text
bot/data/spreadsheets/
```

Raw Gemini text/JSON is stored with the spreadsheet session for short-term auditing and is cleaned after the configured retention period, default 31 days. Generated spreadsheet and report images are treated as temporary delivery files: after the bot uploads them to Discord it removes the local image copies, and maintenance also removes any leftover local images older than the configured local image retention period, default 7 days.

Local normalized screenshot downloads are temporary processing inputs and are cleaned after spreadsheet processing/output.

## Deployment Notes

Recommended production split:

- Bot on Render or another long-running Node host.
- Dashboard on Vercel or another web host.
- Shared database. PostgreSQL and Azure SQL are supported.

For the bot host:

```bash
cd bot
npm install
npm run deploy:commands
npm start
```

For the dashboard host:

```bash
cd dashboard
npm install
npm run build
npm start
```

Use the same `DATABASE_URL` for both apps so bot runtime state and dashboard configuration stay synchronized.

## Troubleshooting

If slash commands are missing:

```bash
cd bot
npm run deploy:commands
```

If Gemini extraction returns poor results:

- Use uncropped, high-resolution screenshots.
- Increase grouping window if screenshots arrive slowly.
- Add own team aliases in the dashboard.
- Use `/spreadsheets preview` or `/spreadsheets test-gemini` to inspect parsed rows.
- Use `/spreadsheets correct-*` commands for field-level fixes.
- Rebuild with `/spreadsheets rebuild` after corrections.

If weekly or monthly reports are empty:

- Confirm the team has processed sessions.
- Confirm the session dates fall in the requested week or month.
- Use `anchor_date` to target an older period.
- Confirm the team ID matches the configured dashboard team.

If automatic reports are not posted:

- Confirm the team output channel is configured.
- Confirm the bot can send messages and attach files in that channel.
- Confirm auto process is enabled, or manually run `/spreadsheets generate`.

If dashboard login fails:

- Verify the Discord OAuth redirect URI.
- Verify the dashboard role ID.
- Verify the community server ID.
- Verify the bot is in the configured server.
- Verify cookies are allowed for the dashboard domain.

## Contributors

- Drago
- Devil
- BlackWing
