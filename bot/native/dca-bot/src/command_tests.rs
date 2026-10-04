//! Every slash and prefix command, dispatched against the fake Discord: nothing may crash, and the replies are the
//! ones the Node bot gave.

use crate::commands;
use crate::flow_tests::*;
use crate::mock_discord::{Call, BOT_ID, GUILD_ID};
use dca_state::stores::*;
use serde_json::{json, Value};
use serenity::all::*;

const ADMIN: &str = "8";
const BAN: &str = "4";
const MEMBER: u64 = 100_000_000_000_000_007;
const MOD: u64 = 100_000_000_000_000_011;
const TARGET: u64 = 100_000_000_000_000_023;

fn opt(name: &str, kind: u8, value: Value) -> Value {
    json!({ "name": name, "type": kind, "value": value })
}

fn sub(name: &str, options: Vec<Value>) -> Value {
    json!({ "name": name, "type": 1, "options": options })
}

fn slash(rig: &Rig, name: &str, options: Vec<Value>, user: u64, perms: &str) -> CommandInteraction {
    let target_member = json!({ "roles": [], "joined_at": "2026-01-01T00:00:00.000000+00:00", "deaf": false, "mute": false, "permissions": "0", "flags": 0 });
    let value = json!({
        "id": rig.mock.next_id().to_string(), "application_id": BOT_ID.to_string(), "type": 2, "token": format!("tok-{}", rig.mock.next_id()), "version": 1,
        "data": { "id": rig.mock.next_id().to_string(), "name": name, "type": 1, "options": options,
                  "resolved": { "users": { TARGET.to_string(): user_json(TARGET, "target") }, "members": { TARGET.to_string(): target_member } } },
        "guild_id": GUILD_ID.to_string(), "channel_id": PANEL.to_string(), "channel": { "id": PANEL.to_string(), "type": 0 },
        "member": member_json(user, "caller", &[], perms), "user": user_json(user, "caller"),
        "app_permissions": "0", "locale": "en-US", "entitlements": [], "attachment_size_limit": 26214400
    });
    serde_json::from_value(value).expect("command json")
}

fn replies(rig: &Rig) -> Vec<Value> {
    rig.mock
        .calls()
        .into_iter()
        .filter(|c: &Call| (c.method == "POST" && c.path.contains("/callback")) || (c.method == "PATCH" && c.path.contains("/@original")) || (c.method == "POST" && c.path.starts_with("/webhooks/")))
        .map(|c| if c.path.contains("/callback") { c.body["data"].clone() } else { c.body })
        .collect()
}

async fn run(rig: &Rig, name: &str, options: Vec<Value>, user: u64, perms: &str) -> String {
    let before = replies(rig).len();
    commands::dispatch_slash(rig.app.clone(), slash(rig, name, options, user, perms)).await;
    let all = replies(rig);
    assert!(all.len() > before, "/{name} gave no reply");
    let text = all[before..].iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n");
    assert!(!text.contains("Error executing this command"), "/{name} failed: {text}");
    text
}

fn text_message(rig: &Rig, content: &str, author: u64, mention: Option<u64>) -> Message {
    let mut m = message_json(rig.mock.next_id(), PANEL, author, content, vec![]);
    m["author"]["bot"] = json!(false);
    if let Some(u) = mention {
        m["mentions"] = json!([{ "id": u.to_string(), "username": "target", "discriminator": "0", "avatar": null, "member": { "roles": [], "joined_at": "2026-01-01T00:00:00.000000+00:00", "deaf": false, "mute": false } }]);
    }
    m["member"] = json!({ "roles": [], "joined_at": "2026-01-01T00:00:00.000000+00:00", "deaf": false, "mute": false, "permissions": ADMIN });
    parse_message(m)
}

async fn run_text(rig: &Rig, content: &str, author: u64, mention: Option<u64>) -> String {
    let before: usize = rig.all_posts();
    commands::dispatch_text(rig.app.clone(), text_message(rig, content, author, mention)).await;
    rig.posts_since(before)
}

impl Rig {
    fn all_posts(&self) -> usize {
        self.mock.calls().into_iter().filter(|c| c.method == "POST" && c.path.ends_with("/messages")).count()
    }
    fn posts_since(&self, n: usize) -> String {
        self.mock.calls().into_iter().filter(|c| c.method == "POST" && c.path.ends_with("/messages")).skip(n).map(|c| c.body.to_string()).collect::<Vec<_>>().join("\n")
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn information_commands() {
    let rig = Rig::new("cmd-info", false).await;
    let help = run(&rig, "help", vec![], MEMBER, "0").await;
    for needle in ["/tickets", "/spreadsheets", "/remindMe"] {
        assert!(help.contains(needle), "/help lists {needle}: {help}");
    }
    assert!(!help.to_lowercase().contains("gemini"), "no mention of the retired Gemini: {help}");
    assert!(run(&rig, "ping", vec![], MEMBER, "0").await.contains("Pong"));
    run_text(&rig, "-ping", MEMBER, None).await;
    assert!(rig.mock.calls_matching("PATCH", "/messages/").iter().any(|c| c.body["content"].as_str().unwrap_or("").contains("Pong")), "the ping message is edited with the result");
    assert!(run(&rig, "invite", vec![], MEMBER, "0").await.len() > 10);
    assert!(run(&rig, "dashboard", vec![], MEMBER, "0").await.len() > 10);
    let whois = run(&rig, "whois", vec![opt("target", 6, json!(TARGET.to_string()))], MEMBER, "0").await;
    assert!(whois.contains(&TARGET.to_string()) && whois.contains("Account Created"), "{whois}");
    let car = run(&rig, "sportscar", vec![], MEMBER, "0").await;
    for needle in ["REV SURGE", "OVERDRIVE", "MEGA TANK", "EXTRA PART"] {
        assert!(car.contains(needle), "{needle}");
    }
    assert!(run(&rig, "garage", vec![], MEMBER, "0").await.contains("Mastery Garage"));
    let counts = run(&rig, "membercount", vec![sub("list", vec![])], MEMBER, "0").await;
    assert!(counts.len() > 20, "{counts}");
}

#[tokio::test(flavor = "multi_thread")]
async fn reminders_are_saved_listed_and_cancelled() {
    let rig = Rig::new("cmd-remind", false).await;
    let short = run(&rig, "remindme", vec![opt("duration", 3, json!("2s")), opt("message", 3, json!("x"))], MEMBER, "0").await;
    assert!(short.contains("valid time"), "{short}");
    let set = run(&rig, "remindme", vec![opt("duration", 3, json!("10m")), opt("message", 3, json!("stretch"))], MEMBER, "0").await;
    assert!(set.contains("Reminder set") && set.contains("10m"), "{set}");
    let list = run(&rig, "reminders", vec![], MEMBER, "0").await;
    assert!(list.contains("stretch"), "{list}");
    assert!(run(&rig, "cancelreminder", vec![opt("id", 4, json!(1))], MEMBER, "0").await.contains("cancelled"));
    assert!(run(&rig, "reminders", vec![], MEMBER, "0").await.contains("no active reminders"));
    assert!(run(&rig, "cancelreminder", vec![opt("id", 4, json!(77))], MEMBER, "0").await.contains("No reminder found"));
}

#[tokio::test(flavor = "multi_thread")]
async fn moderation_commands_check_permissions_and_act() {
    let rig = Rig::new("cmd-mod", false).await;
    let denied = run(&rig, "ban", vec![opt("user", 6, json!(TARGET.to_string())), opt("reason", 3, json!("spam"))], MEMBER, "0").await;
    assert!(denied.contains("permission"), "{denied}");
    assert!(rig.mock.calls_matching("PUT", "/bans/").is_empty(), "nobody was banned");
    let ok = run(&rig, "ban", vec![opt("user", 6, json!(TARGET.to_string())), opt("reason", 3, json!("spam"))], MOD, BAN).await;
    assert!(ok.to_lowercase().contains("ban"), "{ok}");
    assert_eq!(rig.mock.calls_matching("PUT", "/bans/").len(), 1);

    let warned = run(&rig, "warn", vec![opt("user", 6, json!(TARGET.to_string())), opt("reason", 3, json!("rude"))], MOD, ADMIN).await;
    assert!(warned.contains("warn"), "{warned}");
    assert_eq!(list_warnings(&rig.app.store, &TARGET.to_string(), &GUILD_ID.to_string()).await.len(), 1);
    let cleared = run(&rig, "clearwarns", vec![opt("user", 6, json!(TARGET.to_string()))], MOD, ADMIN).await;
    assert!(cleared.contains("Cleared all warnings"), "{cleared}");
    assert!(list_warnings(&rig.app.store, &TARGET.to_string(), &GUILD_ID.to_string()).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn management_commands_report_status() {
    let rig = Rig::new("cmd-manage", false).await;
    let tickets = run(&rig, "tickets", vec![sub("status", vec![])], MOD, ADMIN).await;
    assert!(tickets.contains("Panel Channel"), "{tickets}");
    let denied = run(&rig, "tickets", vec![sub("sync-panel", vec![])], MEMBER, "0").await;
    assert!(denied.to_lowercase().contains("manage") || denied.to_lowercase().contains("permission"), "{denied}");

    let rig2 = sheet_rig_pub("cmd-sheets").await;
    let status = run(&rig2, "spreadsheets", vec![sub("status", vec![opt("team", 3, json!("discord"))])], MOD, ADMIN).await;
    assert!(status.contains("Team: **Discord**") && status.contains("local PaddleOCR"), "{status}");
    let nobody = run(&rig2, "spreadsheets", vec![sub("status", vec![opt("team", 3, json!("discord"))])], MEMBER, "0").await;
    assert!(nobody.contains("do not have access"), "{nobody}");
    let sessions = run(&rig2, "spreadsheets", vec![sub("sessions", vec![opt("team", 3, json!("discord"))])], MOD, ADMIN).await;
    assert!(sessions.contains("No spreadsheet sessions"), "{sessions}");
}

#[tokio::test(flavor = "multi_thread")]
async fn prefix_commands() {
    let rig = Rig::new("cmd-text", false).await;
    rig.mock.set_member_roles(MOD, &[crate::mock_discord::ADMIN_ROLE]);
    let help = run_text(&rig, "-help", MEMBER, None).await;
    assert!(help.contains("warn") && help.contains("temperature"), "{help}");
    assert!(run_text(&rig, "-bam", MEMBER, Some(TARGET)).await.contains(&TARGET.to_string()));
    let warn = run_text(&rig, &format!("-warn <@{TARGET}> being rude"), MOD, Some(TARGET)).await;
    assert!(!warn.contains("permission"), "{warn}\n{:?}", rig.mock.calls().iter().map(|c| format!("{} {}", c.method, c.path)).collect::<Vec<_>>());
    assert!(!warn.is_empty());
    let warnings = run_text(&rig, &format!("-warnings <@{TARGET}>"), MOD, Some(TARGET)).await;
    assert!(warnings.contains("being rude"), "{warnings}");
    assert!(run_text(&rig, "-nonsense", MEMBER, None).await.is_empty(), "unknown commands are ignored");
    assert!(run_text(&rig, "hello there", MEMBER, None).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_guild_is_readable() {
    let rig = Rig::new("cmd-guild", false).await;
    let guild = rig.app.http.get_guild(GuildId::new(GUILD_ID)).await;
    assert!(guild.is_ok(), "{:?}", guild.err());
}

// ------------------------------------------------------------------------------- community managers

const COMMUNITY_CHANNEL: u64 = 840_000_000_000_000_001;
const ROLE_PING: u64 = 840_000_000_000_000_010;

fn member_for_event(id: u64, name: &str) -> Member {
    let mut v = member_json(id, name, &[], "0");
    v["guild_id"] = json!(GUILD_ID.to_string());
    serde_json::from_value(v).expect("member json")
}

#[tokio::test(flavor = "multi_thread")]
async fn welcome_and_leave_messages() {
    let rig = Rig::new("welcome", false).await;
    rig.mock.register_channel(COMMUNITY_CHANNEL, 0, "welcome");
    dca_state::config::update_config(&rig.app.store, |c| {
        c.bot.community_guild_id = GUILD_ID.to_string();
        c.welcome.enabled = true;
        c.welcome.channel_id = COMMUNITY_CHANNEL.to_string();
        c.welcome.message = "Welcome {member} to {server}!".into();
        c.leave.enabled = true;
        c.leave.channel_id = COMMUNITY_CHANNEL.to_string();
        c.leave.message = "{username} left {server}.".into();
    })
    .await
    .unwrap();
    let joined = member_for_event(TARGET, "newbie");
    crate::managers::welcome::on_member_add(&rig.app, &joined).await;
    let posts = rig.mock.messages_in(COMMUNITY_CHANNEL);
    assert_eq!(posts.len(), 1);
    let text = posts[0]["content"].as_str().unwrap();
    assert!(text.contains(&format!("<@{TARGET}>")) && text.contains("Test Guild"), "{text}");

    crate::managers::welcome::on_member_remove(&rig.app, GuildId::new(GUILD_ID), &joined.user, Some(&joined)).await;
    let posts = rig.mock.messages_in(COMMUNITY_CHANNEL);
    assert_eq!(posts.len(), 2);
    let text = posts[1]["content"].as_str().unwrap();
    assert!(text.contains("newbie") && !text.contains(&format!("<@{TARGET}>")), "a leave message never pings: {text}");
    // The old bot logged both.
    let logs = dca_state::stores::list_bot_logs(&rig.app.store, 10, "").await;
    assert!(logs.iter().any(|l| l.title == "Member Joined") && logs.iter().any(|l| l.title == "Member Left"), "{logs:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn reaction_roles_are_posted_and_handed_out() {
    let rig = Rig::new("reactions", false).await;
    rig.mock.register_channel(COMMUNITY_CHANNEL, 0, "roles");
    dca_state::config::update_config(&rig.app.store, |c| {
        c.reaction_roles = vec![dca_state::config::ReactionRoleGroup {
            id: "pings".into(),
            name: "Ping roles".into(),
            enabled: true,
            channel_id: COMMUNITY_CHANNEL.to_string(),
            message: "React for pings".into(),
            options: vec![dca_state::config::ReactionOption { emoji: "\u{1f514}".into(), role_id: ROLE_PING.to_string(), label: "Pings".into() }],
            ..Default::default()
        }];
    })
    .await
    .unwrap();
    let (config, results) = crate::managers::reaction_roles::sync_reaction_roles(&rig.app, false).await.expect("sync");
    assert_eq!(results.len(), 1);
    let message_id = config.reaction_roles[0].message_id.clone();
    assert!(!message_id.is_empty(), "the message id is saved: {results:?}");
    assert_eq!(rig.mock.messages_in(COMMUNITY_CHANNEL).len(), 1);
    assert!(!rig.mock.calls_matching("PUT", "/reactions/").is_empty(), "the bot added the reaction");

    // Syncing again edits nothing new: still one message.
    crate::managers::reaction_roles::sync_reaction_roles(&rig.app, false).await.unwrap();
    assert_eq!(rig.mock.messages_in(COMMUNITY_CHANNEL).len(), 1);

    let reaction: Reaction = serde_json::from_value(json!({
        "user_id": TARGET.to_string(), "channel_id": COMMUNITY_CHANNEL.to_string(), "message_id": message_id, "guild_id": GUILD_ID.to_string(),
        "emoji": { "id": null, "name": "\u{1f514}" }, "member": member_json(TARGET, "newbie", &[], "0"), "burst": false, "burst_colours": [], "type": 0
    }))
    .unwrap();
    crate::managers::reaction_roles::handle_reaction(&rig.app, &reaction, true).await;
    assert_eq!(rig.mock.calls_matching("PUT", &format!("/members/{TARGET}/roles/{ROLE_PING}")).len(), 1, "role granted");
    crate::managers::reaction_roles::handle_reaction(&rig.app, &reaction, false).await;
    assert_eq!(rig.mock.calls_matching("DELETE", &format!("/members/{TARGET}/roles/{ROLE_PING}")).len(), 1, "role removed");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_recruitment_panel_is_posted_once_and_member_counts_are_kept_up() {
    let rig = Rig::new("panels", false).await;
    let first = crate::managers::recruitment::ensure_recruitment_panel(&rig.app).await;
    assert!(!first.skipped, "{}", first.reason);
    assert!(first.created);
    let panel = rig.mock.messages_in(PANEL);
    assert_eq!(panel.len(), 1);
    assert!(panel[0]["components"].to_string().contains("recruitment:apply"), "{}", panel[0]);
    let second = crate::managers::recruitment::ensure_recruitment_panel(&rig.app).await;
    assert!(!second.created && rig.mock.messages_in(PANEL).len() == 1, "no duplicate panel");
    assert!(rig.mock.calls_matching("PATCH", "/messages/").is_empty(), "an unchanged panel is not edited");

    rig.mock.register_channel(COMMUNITY_CHANNEL, 0, "teams");
    dca_state::config::update_config(&rig.app.store, |c| {
        c.member_counts.enabled = true;
        c.member_counts.channel_id = COMMUNITY_CHANNEL.to_string();
    })
    .await
    .unwrap();
    let sync = crate::managers::member_count::sync_member_count_message(&rig.app, true, "").await;
    assert!(!sync.skipped, "{}", sync.reason);
    assert_eq!(rig.mock.messages_in(COMMUNITY_CHANNEL).len(), 1);
    let again = crate::managers::member_count::sync_member_count_message(&rig.app, true, "").await;
    assert!(!again.created, "the same message is edited");
    assert_eq!(rig.mock.messages_in(COMMUNITY_CHANNEL).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn spreadsheet_commands_correct_rebuild_and_report() {
    if !models_ready() {
        eprintln!("skipping: PaddleOCR models not downloaded");
        return;
    }
    let rig = sheet_rig_pub("cmd-sheet-flow").await;
    submit_bench_pages(&rig).await;
    let team = vec![opt("team", 3, json!("discord"))];
    let with = |mut base: Vec<Value>, extra: Vec<Value>| {
        base.extend(extra);
        base
    };

    let sessions = run(&rig, "spreadsheets", vec![sub("sessions", team.clone())], MOD, ADMIN).await;
    assert!(sessions.contains("pending") && sessions.contains("5 image"), "{sessions}");
    let id = list_sessions(&rig.app.store, &Default::default()).await[0].id.clone();

    let generated = run(&rig, "spreadsheets", vec![sub("generate", with(team.clone(), vec![opt("session_id", 3, json!(id))]))], MOD, ADMIN).await;
    assert!(generated.contains("Double-time Dilemma"), "{generated}");
    // The generated files were attached to the reply and the output channel got the final post.
    assert!(rig.mock.calls().iter().any(|c| c.method == "PATCH" && c.files.iter().any(|f| f.ends_with(".xlsx"))), "xlsx attached to the reply");
    assert!(!rig.mock.messages_in(OUTPUT_CHANNEL).is_empty(), "posted to the output channel");

    let summary = run(&rig, "spreadsheets", vec![sub("summary", with(team.clone(), vec![opt("session_id", 3, json!(id))]))], MOD, ADMIN).await;
    assert!(summary.contains("Players") || summary.contains("players"), "{summary}");

    // Fix a name and a placement: the session and its outputs follow.
    let fixed =
        run(&rig, "spreadsheets", vec![sub("correct-name", with(team.clone(), vec![opt("session_id", 3, json!(id)), opt("row", 4, json!(1)), opt("value", 3, json!("Corrected Name"))]))], MOD, ADMIN)
            .await;
    assert!(fixed.contains("Player name correction applied"), "{fixed}");
    let session = get_session(&rig.app.store, &id).await.unwrap();
    assert_eq!(session.players.iter().find(|p| p.rank == 1).unwrap().player_name, "Corrected Name");
    assert_eq!(session.corrections.len(), 1);
    let again = run(&rig, "spreadsheets", vec![sub("rebuild", with(team.clone(), vec![opt("session_id", 3, json!(id))]))], MOD, ADMIN).await;
    assert!(again.contains("Rebuilt"), "{again}");
    let session = get_session(&rig.app.store, &id).await.unwrap();
    assert_eq!(session.players.iter().find(|p| p.rank == 1).unwrap().player_name, "Corrected Name", "a rebuild replays the correction");

    // Weekly report for the week of the session, and the chart / file commands.
    let weekly = run(&rig, "spreadsheets", vec![sub("weekly", team.clone())], MOD, ADMIN).await;
    assert!(weekly.contains("Weekly report"), "{weekly}");
    assert!(run(&rig, "spreadsheets", vec![sub("chart", with(team.clone(), vec![opt("session_id", 3, json!(id))]))], MOD, ADMIN).await.contains("Chart for"));
    assert!(run(&rig, "spreadsheets", vec![sub("file", with(team.clone(), vec![opt("session_id", 3, json!(id))]))], MOD, ADMIN).await.contains("Spreadsheet for"));
    let bad = run(&rig, "spreadsheets", vec![sub("correct-name", with(team, vec![opt("session_id", 3, json!("race-nope")), opt("row", 4, json!(1)), opt("value", 3, json!("x"))]))], MOD, ADMIN).await;
    assert!(bad.contains("No processed session"), "{bad}");
}

#[tokio::test(flavor = "multi_thread")]
async fn youtube_uploads_are_announced_once() {
    use std::sync::{Arc, Mutex};
    let rig = Rig::new("youtube", false).await;
    rig.mock.register_channel(COMMUNITY_CHANNEL, 0, "videos");
    let feed = Arc::new(Mutex::new(String::new()));
    let entry = |id: &str, title: &str, when: &str| {
        format!("<entry><id>yt:video:{id}</id><title>{title}</title><link rel=\"alternate\" href=\"https://www.youtube.com/watch?v={id}\"/><published>{when}</published></entry>")
    };
    let now = chrono::Utc::now();
    let doc = |entries: String| format!("<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\">{entries}</feed>");
    *feed.lock().unwrap() = doc(entry("old1", "Old", &(now - chrono::Duration::hours(30)).to_rfc3339()));
    let served = feed.clone();
    let router = axum::Router::new().route(
        "/feeds/videos.xml",
        axum::routing::get(move || {
            let body = served.lock().unwrap().clone();
            async move { ([("content-type", "application/atom+xml")], body) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(listener, router).await.ok() });
    std::env::set_var("DCA_YOUTUBE_FEED_BASE", &base);

    dca_state::config::update_config(&rig.app.store, |c| {
        c.youtube.enabled = true;
        c.youtube.default_channel_id = COMMUNITY_CHANNEL.to_string();
        c.youtube.announcement_template = "**{name}** uploaded a new video!\n{url}".into();
        c.youtube.feeds = vec![dca_state::config::YoutubeFeed { id: "UCtest".into(), name: "DCA".into(), channel_id: COMMUNITY_CHANNEL.to_string(), enabled: true, ..Default::default() }];
    })
    .await
    .unwrap();

    let first = crate::managers::youtube::check_feeds(&rig.app).await;
    assert!(matches!(first, crate::managers::youtube::CheckOutcome::Results(ref r) if r[0].initialized), "the first look only records what exists");
    assert!(rig.mock.messages_in(COMMUNITY_CHANNEL).is_empty(), "nothing is announced for existing videos");

    *feed.lock().unwrap() = doc(format!("{}{}", entry("new1", "Fresh upload", &now.to_rfc3339()), entry("old1", "Old", &(now - chrono::Duration::hours(30)).to_rfc3339())));
    let second = crate::managers::youtube::check_feeds(&rig.app).await;
    assert!(
        matches!(second, crate::managers::youtube::CheckOutcome::Results(ref r) if r[0].posted),
        "{:?}",
        match second {
            crate::managers::youtube::CheckOutcome::Results(r) => format!("{:?}", r),
            _ => String::new(),
        }
    );
    let posts = rig.mock.messages_in(COMMUNITY_CHANNEL);
    assert_eq!(posts.len(), 1, "{:?}", rig.mock.calls().iter().map(|c| format!("{} {}", c.method, c.path)).collect::<Vec<_>>());
    assert!(posts[0]["content"].as_str().unwrap().contains("**DCA** uploaded a new video!") && posts[0]["content"].as_str().unwrap().contains("watch?v=new1"), "{}", posts[0]);

    crate::managers::youtube::check_feeds(&rig.app).await;
    assert_eq!(rig.mock.messages_in(COMMUNITY_CHANNEL).len(), 1, "never announced twice");
    std::env::remove_var("DCA_YOUTUBE_FEED_BASE");
}
