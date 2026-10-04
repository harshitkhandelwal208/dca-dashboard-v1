//! Gateway events: the old `index.js` handlers plus `events/*.js`.

use crate::app::App;
use crate::commands;
use crate::managers::{ban_panel, member_count, reaction_roles, recruitment, reminders, spreadsheet, team_roles, welcome, youtube};
use crate::responder::{Ix, ReplyData, Responder};
use serenity::all::*;
use serenity::async_trait;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

const BUTTON_ACTION_TTL: Duration = Duration::from_secs(15 * 60);
const PING_SETTINGS_CHANNEL: u64 = 839907517663936612;
const PING_SETTINGS_FIRST_LINE: &str = "It's not necessary to join every time it's reset.";
const PING_SETTINGS_IMAGE: &str = "https://cdn.discordapp.com/attachments/1341563215611433035/1349012518915539004/20230504_180004-1.jpg?ex=67d18d4f&is=67d03bcf&hm=37688a5b7897d910b2e63227b006459c68b4e74c9bd7d48b5c50f81451766c73&";

pub struct Handler {
    pub app: Arc<App>,
}

/// The key a button's in-flight lock is registered under (same grouping as the old `buttonActionKey`).
fn button_action_key(i: &ComponentInteraction) -> String {
    let id = i.data.custom_id.as_str();
    let guild = i.guild_id.map(|g| g.to_string()).unwrap_or_default();
    let (channel, user) = (i.channel_id, i.user.id);
    if id == "recruitment:apply" {
        format!("button:{guild}:{user}:{id}")
    } else if id == "recruitment:claim" {
        format!("button:{guild}:{channel}:{id}")
    } else if id == "recruitment:close" {
        format!("button:{guild}:{channel}:{user}:{id}")
    } else if id.starts_with("recruitment:close-team:") {
        format!("button:{guild}:{channel}:recruitment:close-team")
    } else if id.starts_with("recruitment:event:") {
        format!("button:{guild}:{user}:{id}")
    } else if id.starts_with("recruitment:tutorial:") {
        format!("button:{guild}:{channel}:{user}:{id}")
    } else if id.starts_with("welcome-team:") {
        format!("button:{guild}:{id}")
    } else {
        format!("button:{}", i.id)
    }
}

struct ButtonLock {
    app: Arc<App>,
    key: String,
}

impl Drop for ButtonLock {
    fn drop(&mut self) {
        self.app.button_locks.lock().unwrap().remove(&self.key);
    }
}

async fn handle_button(app: Arc<App>, i: ComponentInteraction) {
    let key = button_action_key(&i);
    let r = Responder::new(app.http.clone(), Ix::Comp(i.clone()));
    if !app.button_locks.lock().unwrap().insert(key.clone()) {
        let _ = r.reply(ReplyData::text("That button action is already being processed.").ephemeral()).await;
        return;
    }
    let lock = ButtonLock { app: app.clone(), key: key.clone() };
    // Safety valve: release the lock after the TTL even if the handler hangs.
    {
        let app = app.clone();
        let key = key.clone();
        tokio::spawn(async move {
            tokio::time::sleep(BUTTON_ACTION_TTL).await;
            app.button_locks.lock().unwrap().remove(&key);
        });
    }
    let custom_id = i.data.custom_id.clone();
    if recruitment::handle_component(app.clone(), r, custom_id.clone()).await {
        // `collect_license` and friends keep running detached; the lock only covers the click itself.
        drop(lock);
        return;
    }
    let r = Responder::new(app.http.clone(), Ix::Comp(i));
    if welcome::handle_welcome_team_button(&app, &r, &custom_id).await {
        drop(lock);
    }
}

async fn handle_select(app: Arc<App>, i: ComponentInteraction) {
    let values = match &i.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => values.clone(),
        _ => return,
    };
    if i.data.custom_id == "select_car" {
        let r = Responder::new(app.http.clone(), Ix::Comp(i));
        commands::mastery::handle_select(&app, &r, &values).await;
    }
}

async fn ping_settings(app: &App) {
    let channel = ChannelId::new(PING_SETTINGS_CHANNEL);
    let Ok(messages) = channel.messages(&app.http, GetMessages::new().limit(10)).await else {
        tracing::info!("Ping settings channel not found or not readable.");
        return;
    };
    if messages.iter().any(|m| m.content.starts_with(PING_SETTINGS_FIRST_LINE) || !m.embeds.is_empty()) {
        return;
    }
    let embed = CreateEmbed::new()
        .title("Ping Settings")
        .description(format!("{PING_SETTINGS_FIRST_LINE}\nIf you are in doubt about what you will get pinged for, check your roles. You should have some of these:"))
        .colour(Colour::new(0x3498db))
        .image(PING_SETTINGS_IMAGE)
        .footer(CreateEmbedFooter::new("Scroll to the top \u{261d}"));
    if let Err(error) = channel.send_message(&app.http, CreateMessage::new().embed(embed)).await {
        tracing::error!("Error sending ping settings message: {error}");
    }
}

async fn run_ready_tasks(app: Arc<App>) {
    tracing::info!("Syncing dashboard-managed reaction role messages...");
    match tokio::time::timeout(Duration::from_secs(20), reaction_roles::sync_reaction_roles(&app, false)).await {
        Ok(Ok((_, results))) => tracing::info!("Reaction role sync done ({} messages).", results.iter().filter(|r| r.skipped != Some(true)).count()),
        Ok(Err(error)) => tracing::error!("Reaction role script error: {error}"),
        Err(_) => tracing::error!("Reaction role script error: timeout after 20s"),
    }

    tracing::info!("Syncing recruitment Apply panel...");
    match tokio::time::timeout(Duration::from_secs(20), recruitment::ensure_recruitment_panel(&app)).await {
        Ok(sync) if sync.skipped => tracing::info!("Recruitment panel sync skipped: {}", sync.reason),
        Ok(sync) => tracing::info!("Recruitment panel ready in {} ({}).", sync.channel_id, sync.message_id),
        Err(_) => tracing::error!("Recruitment panel sync error: timeout after 20s"),
    }

    tracing::info!("Syncing member count message...");
    match tokio::time::timeout(Duration::from_secs(20), member_count::sync_member_count_message(&app, true, "")).await {
        Ok(sync) if sync.skipped => tracing::info!("Member count sync skipped: {}", sync.reason),
        Ok(sync) => tracing::info!("Member count message ready in {} ({}).", sync.channel_id, sync.message_id),
        Err(_) => tracing::error!("Member count sync error: timeout after 20s"),
    }

    ping_settings(&app).await;

    youtube::start_notifier(app.clone());
    team_roles::start_scheduler(app.clone());
    spreadsheet::start_scheduler(app.clone());
    reminders::start_scheduler(app.clone());

    let ban_app = app.clone();
    tokio::spawn(async move {
        let sync = ban_panel::sync_recruitment_ban_list(&ban_app, false).await;
        if sync.skipped {
            tracing::info!("Recruitment ban list sync skipped: {}", sync.reason);
        }
    });

    recruitment::start_panel_sweeper(app.clone());

    // Panel self-heal (the old keep-alive interval).
    let heal = app.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        tick.tick().await;
        loop {
            tick.tick().await;
            let _ = recruitment::ensure_recruitment_panel(&heal).await;
        }
    });
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        let app = self.app.clone();
        app.http.set_application_id(ready.application.id);
        *app.bot_id.write().unwrap() = Some(ready.user.id);
        *app.bot_tag.write().unwrap() = crate::util::display_tag(&ready.user.into());
        // Gateway reconnects fire `ready` again: only the first one starts the background tasks.
        if app.ready.swap(true, Ordering::SeqCst) {
            return;
        }
        tracing::info!("========================\nLogged in as {}\n========================", app.bot_tag.read().unwrap());
        tokio::spawn(run_ready_tasks(app));
    }

    async fn resume(&self, _ctx: Context, _event: ResumedEvent) {
        self.app.ready.store(true, Ordering::SeqCst);
    }

    async fn message(&self, _ctx: Context, message: Message) {
        let app = self.app.clone();
        // Applicants uploading to an Apply session are picked up by the collector waiting on this channel.
        if !message.author.bot {
            let sender = app.collectors.lock().unwrap().get(&(message.channel_id, message.author.id)).cloned();
            if let Some(sender) = sender {
                let _ = sender.send(message.clone());
            }
        }
        // The spreadsheet channels only matter for screenshots: plain chat costs nothing.
        if !message.attachments.is_empty() && !message.author.bot && message.guild_id.is_some() {
            let app = app.clone();
            let message = message.clone();
            tokio::spawn(async move {
                spreadsheet::handle_message(&app, &message).await;
            });
        }
        if message.content.starts_with(commands::PREFIX) && !message.author.bot {
            tokio::spawn(commands::dispatch_text(app, message));
        }
    }

    async fn interaction_create(&self, _ctx: Context, interaction: Interaction) {
        let app = self.app.clone();
        match interaction {
            Interaction::Command(cmd) => {
                tokio::spawn(commands::dispatch_slash(app, cmd));
            }
            Interaction::Component(i) => match i.data.kind {
                ComponentInteractionDataKind::Button => {
                    tokio::spawn(handle_button(app, i));
                }
                ComponentInteractionDataKind::StringSelect { .. } => {
                    tokio::spawn(handle_select(app, i));
                }
                _ => {}
            },
            Interaction::Modal(m) => {
                let r = Responder::new(app.http.clone(), Ix::Modal(m.clone()));
                tokio::spawn(async move {
                    recruitment::handle_component(app, r, m.data.custom_id.clone()).await;
                });
            }
            _ => {}
        }
    }

    async fn reaction_add(&self, _ctx: Context, reaction: Reaction) {
        let app = self.app.clone();
        tokio::spawn(async move { reaction_roles::handle_reaction(&app, &reaction, true).await });
    }

    async fn reaction_remove(&self, _ctx: Context, reaction: Reaction) {
        let app = self.app.clone();
        tokio::spawn(async move { reaction_roles::handle_reaction(&app, &reaction, false).await });
    }

    async fn guild_member_addition(&self, _ctx: Context, member: Member) {
        let app = self.app.clone();
        tokio::spawn(async move { welcome::on_member_add(&app, &member).await });
    }

    async fn guild_member_removal(&self, _ctx: Context, guild_id: GuildId, user: User, member: Option<Member>) {
        let app = self.app.clone();
        tokio::spawn(async move { welcome::on_member_remove(&app, guild_id, &user, member.as_ref()).await });
    }
}
