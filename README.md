# DCA Bot Suite

DCA Bot Suite contains two deployable applications that share one Discord bot token and one state store:

- `bot/` - the Discord bot runtime, slash commands, recruitment tickets, reaction roles, team counts, YouTube checks, OCR spreadsheet generation, and automatic team-event reports.
- `dashboard/` - the React and Express dashboard used to configure servers, channels, roles, tickets, member counts, feeds, spreadsheets, and bot logging.

Both apps can use the same Postgres database through `DATABASE_URL`. If no database URL is provided, local JSON files in `bot/data/` are used for development.

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
- A shared Postgres database for production, recommended for Render and Vercel.

The spreadsheet system does not require native OCR or office binaries by default. It uses:

- `tesseract.js` for OCR.
- `sharp` for screenshot normalization.
- `exceljs` for XLSX files.

Leave the dashboard fields `Tesseract path`, `ImageMagick path`, and `LibreOffice path` blank unless you specifically want to use installed native binaries.

Optional native overrides:

```env
TESSERACT_PATH=tesseract
IMAGEMAGICK_PATH=magick
LIBREOFFICE_PATH=soffice
```

ImageMagick is only used for image preprocessing when configured. LibreOffice is only a fallback because XLSX files are written directly by Node.

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
DATABASE_URL=postgres_connection_string_shared_with_dashboard
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
DATABASE_URL=same_postgres_connection_string_as_bot
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
- Recruitment server - recruitment panel, ticket threads, recruiter roles, ban list, tutorial uploads, and recruitment logs.

The dashboard Spreadsheet page can choose monitored channels, output channels, and access roles from both the community and recruitment servers.

## Recruitment Tickets

The recruitment system posts an Apply button. Applicants answer prompts and upload screenshots. Recruiters manage the ticket using slash commands and buttons.

Main commands:

```text
/tickets setup
/tickets sync-panel
/tickets status
/tickets logs
/tickets claim
/tickets close
/tickets add
/tickets massadd
/tickets remove
/tickets rename
/tickets tutorial
/tickets archive
/tickets delete
/invite
/ban
```

Ticket close outcomes are configured in the dashboard. Accepted recruits can trigger member-count updates and delayed role assignment in the recruitment and community servers.

## Team Counts And Roles

Member count teams are configured in the dashboard Members page. Each team can have:

- Display name.
- Division.
- Player count.
- Recruitment status.
- Recruitment server role assignment.
- Community server role assignment after rules are accepted.
- Auto-assignment delay.
- Aliases for matching OCR and recruitment data.

Useful commands:

```text
/membercount set
/membercount sync
/teamcount
/updatecount
```

## Spreadsheet OCR

The spreadsheet system watches configured team channels for image attachments. When screenshots are posted, the bot groups images into a pending session for the configured grouping window. If auto-processing is enabled, the session is processed after the window closes.

Dashboard Spreadsheet team settings:

- Enabled - enables capture for that team.
- Monitored channel - screenshot input channel, from either server.
- Output channel - where generated files and reports are posted, from either server.
- Team access role - role allowed to use spreadsheet commands, from either server.
- Own team aliases - names used to identify own-team rows in OCR output.
- Auto process - automatically process sessions after the grouping window.

Global Spreadsheet settings:

- Grouping window - minutes to group screenshots from the same user.
- Output format - `xlsx` or `fods`.
- Keep source images - keeps downloaded screenshots in `bot/data/spreadsheets/`.
- Tesseract path - leave blank for `tesseract.js`.
- ImageMagick path - leave blank for `sharp`.
- LibreOffice path - leave blank for direct `exceljs` XLSX writing.
- OCR language and PSM modes - passed to OCR.

## Spreadsheet Commands

```text
/spreadsheets status team:<team>
/spreadsheets sessions team:<team>
/spreadsheets generate team:<team> [session_id] [rerun_ocr]
/spreadsheets summary team:<team> [session_id]
/spreadsheets weekly team:<team> [anchor_date]
/spreadsheets monthly team:<team> [anchor_date]
/spreadsheets correct team:<team> session_id:<id> row:<rank> field:<field> value:<value>
/spreadsheets rebuild team:<team> session_id:<id>
/spreadsheets file team:<team> [session_id]
/spreadsheets chart team:<team> [session_id]
```

`anchor_date` uses `YYYY-MM-DD`. If omitted, weekly and monthly reports use the date of the latest processed session.

## Automatic Weekly And Monthly Reports

Every processed team-event session now rebuilds two aggregate reports for the team:

- Weekly report.
- Monthly report.

If the team has an output channel, the bot posts:

- The processed event summary.
- The event XLSX file.
- The event chart.
- The rebuilt weekly XLSX report.
- The rebuilt monthly XLSX report.

Report rules:

- Every processed session in the period is treated as one team event.
- Event columns use the OCR-detected team event name from the screenshot when available.
- Missing players receive a score of `0` for that event.
- The period max score is the sum of each event's max score.
- `%kill` is `total / max`.
- `#KAB` is the number of events where the player ranked above every opponent.
- If no opponent rows were detected for an event, event max score is used as a fallback for `#KAB`.
- Weekly reports include the team event names in both the spreadsheet columns and the Details sheet.
- Monthly reports use the same scoring rules over the calendar month.

The generated workbooks contain:

- `Report` sheet with rank, name, event scores, `%kill`, total, max, `#KAB`, missed events, and attended events.
- `Details` sheet with period metadata, report rules, event names, dates, session IDs, max scores, and players parsed.

## OCR Correction Workflow

If OCR reads a player name, rank, points, score, or team type incorrectly:

```text
/spreadsheets correct team:<team> session_id:<id> row:<rank> field:<field> value:<value>
```

Corrections are saved on the session and outputs are rebuilt. Weekly and monthly reports will reflect corrected session data the next time they are generated or when another event is processed.

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
- Spreadsheets - OCR settings, team channels, output channels, roles, and report setup.
- Members - member count and role assignment settings.
- Logs - combined bot and recruitment logs.
- Server - community and recruitment server IDs, dashboard role, recruiter role, manager role, command log channel, and dashboard URL.

## State Storage

With `DATABASE_URL`, state is stored in the configured Postgres table, default:

```env
STATE_TABLE_NAME=dca_bot_state
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

Tesseract language cache is written under:

```text
bot/data/tesseract-cache/
```

## Deployment Notes

Recommended production split:

- Bot on Render or another long-running Node host.
- Dashboard on Vercel or another web host.
- Shared Postgres database.

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

If OCR returns poor results:

- Use uncropped, high-resolution screenshots.
- Increase grouping window if screenshots arrive slowly.
- Add own team aliases in the dashboard.
- Use `/spreadsheets correct` for field-level fixes.
- Try PSM modes `6,11` first.

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
