# Live test checklist (test server "DCa Bot Testing2")

Run the bot with a test token against a server that mirrors your channel layout. The checklist below assumes the channel layout of that server; adapt the channel names to yours. All settings come from the dashboard config (`dashboardConfig`), so you can change channels and roles there.

Sample files: `bot/fixtures/samples/` (five team-event screenshots of one event, a result screen, a second licence) and `bot/fixtures/guides/` (a licence and a team-event screenshot).

## 0. Start-up (check the bot's log)

- `Registered 23 global command(s)`, `Reaction role sync done (2 messages)`, `Recruitment panel ready`, `Member count message ready`.
- `GET /health` -> `{"status":"healthy","discordStatus":"connected","ocr":"ready"}`.
- Posted by the bot at start: the Apply panel in `#prospect`, the member count message, the two reaction-role messages.

## 1. Recruitment

1. `#prospect` -> **Apply!** -> upload `driver-license.jpg` in the channel. Expect: your upload is deleted, "Driver's license captured. Do you have team event score screenshots to add?" with Yes/No. The screenshot is mirrored to the configured DM user.
2. **No** -> a thread appears in `#prospect` with the application embed and **Claim Ticket** / **Close Ticket**. You get "Your application ticket has been created: <#thread>".
3. Apply again (after closing, or with a second account) and upload a licence **and** a team-event screenshot in one message: the ticket is created at once, no question.
4. Apply and upload a non-game picture: you get the "Correct driver's license screenshot" guide (attempt 1/3). Upload a real one: it goes through.
5. In the ticket: **Claim**, then `/tickets add user:`, `/tickets remove user:`, `/tickets massadd users:`, `/tickets rename name:`, `/tickets screenshot-list type:license`, `screenshot-add type:event image:`, `screenshot-change type:license index:1 image:`, `screenshot-remove type:event index:1`.
6. **Close Ticket** -> choose **Discord 3™**: the thread is locked + archived, `#recruit-archive` gets the "Recruitment Closing Log" embed at once (user, team joined, screenshot counts, licence image, "Reading the screenshots...") and a few seconds later the same embed is edited with the in-game name, previous team, garage power and event rank/points/score, the member count message goes up by one, and about a minute later the applicant gets the **Discord 3™ Team** role.
7. Close another with **Rejected**: no log embed, only the mod-log entry.
8. `/tickets status`, `/tickets logs`, `/tickets sync-panel`, `/tickets sync-banlist`, `/tickets setup channel:`, `/tickets archive`, `/tickets delete`.

The screenshots are read once, in the background, at close (accepted tickets only).

## 2. Team-event spreadsheets (`#te-spreadsheet` -> output `#te-podiums-3`)

1. Post the five `IMG_2026…jpg` pages and the `Screenshot_…Hill_Climb_Racing_2.jpg` result screen in `#te-spreadsheet` (any order, within a minute). Chat messages in the channel are ignored.
2. About a minute after the last image the bot posts, in `#te-podiums-3`, the XLSX, the spreadsheet image and the chart: "Double-time Dilemma", 96 players, Discord 3312 vs LIBERTY 1210.
3. `/spreadsheets status team:discord3`, `sessions`, `summary`, `file`, `chart`, `correct-name row:1 value:...`, `correct-placement`, `correct-points`, `correct-event-name`, `rebuild`, `weekly`, `monthly`, `regenerate-weekly`, `regenerate-monthly`, `test-grouping`, `preview`, `generate rerun_ocr:true`.

## 3. Roles, community and utilities

- Reaction roles: react in `#server-rules` (check -> **Read Rules**) and `#ping-settings` (thumbs up -> **PE call**, bell -> **GC call**); remove the reaction and the role goes.
- Welcome / leave: have a second account join and leave (messages in `#welcome` and `#members-left`).
- `/membercount list`, `/membercount set team: players: status:`, `/membercount sync`, `/teamcount team: players:`, `/updatecount team: field: value:`.
- `/yt list`, `/yt check`.
- `/remindme duration:10s message:test`, `/reminders`, `/cancelreminder`.
- `/whois`, `/help`, `/dashboard`, `/invite`, `/sportscar`, `/garage`, `/teameventsummary`, `/roles add user: role:` / `remove`, `/top3te`, `/top3km` (need a screenshot attachment and a ping role).
- Moderation (use a throw-away account): `/warn`, `/clearwarns`, `/ban user: reason:`, `/snap ban|kick user:`; prefix `-warn @u`, `-warnings @u`, `-clearwarns @u`, `-mute @u 1m`, `-kick`, `-unban`, `-clean 5`, `-snapban`.
- Prefix: `-help`, `-bam @user`, `-temperature London`, `-team`, `-yt`.

## 4. Dashboard

`http://localhost:3939/dashboard` loads; signing in needs `DISCORD_CLIENT_SECRET` and a redirect URI registered for the application.
