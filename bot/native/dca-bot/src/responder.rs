//! One reply API over slash commands, buttons/select menus and modal submits that tracks whether the interaction
//! was already deferred/replied to (what discord.js did implicitly with `interaction.deferred` / `.replied`).

use crate::util::BotResult;
use serenity::all::*;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub enum Ix {
    Cmd(CommandInteraction),
    Comp(Box<ComponentInteraction>),
    Modal(ModalInteraction),
}

#[derive(Default, Clone)]
pub struct ReplyData {
    pub content: Option<String>,
    pub embeds: Vec<CreateEmbed>,
    /// `None` leaves components untouched (edit) / empty (new message); `Some(vec![])` clears them.
    pub components: Option<Vec<CreateActionRow>>,
    pub files: Vec<CreateAttachment>,
    pub ephemeral: bool,
    pub allowed_mentions: Option<CreateAllowedMentions>,
}

impl ReplyData {
    pub fn text(content: impl Into<String>) -> ReplyData {
        ReplyData { content: Some(content.into()), ..Default::default() }
    }
    pub fn ephemeral(mut self) -> ReplyData {
        self.ephemeral = true;
        self
    }
    pub fn embed(mut self, embed: CreateEmbed) -> ReplyData {
        self.embeds.push(embed);
        self
    }
    pub fn components(mut self, rows: Vec<CreateActionRow>) -> ReplyData {
        self.components = Some(rows);
        self
    }
    pub fn file(mut self, file: CreateAttachment) -> ReplyData {
        self.files.push(file);
        self
    }
}

#[derive(Default)]
struct State {
    deferred: bool,
    replied: bool,
}

pub struct Responder {
    pub ix: Ix,
    http: Arc<Http>,
    state: Mutex<State>,
}

impl Responder {
    pub fn new(http: Arc<Http>, ix: Ix) -> Responder {
        Responder { ix, http, state: Mutex::new(State::default()) }
    }

    pub fn user(&self) -> &User {
        match &self.ix {
            Ix::Cmd(i) => &i.user,
            Ix::Comp(i) => &i.user,
            Ix::Modal(i) => &i.user,
        }
    }

    pub fn guild_id(&self) -> Option<GuildId> {
        match &self.ix {
            Ix::Cmd(i) => i.guild_id,
            Ix::Comp(i) => i.guild_id,
            Ix::Modal(i) => i.guild_id,
        }
    }

    pub fn channel_id(&self) -> ChannelId {
        match &self.ix {
            Ix::Cmd(i) => i.channel_id,
            Ix::Comp(i) => i.channel_id,
            Ix::Modal(i) => i.channel_id,
        }
    }

    pub fn member(&self) -> Option<&Member> {
        match &self.ix {
            Ix::Cmd(i) => i.member.as_deref(),
            Ix::Comp(i) => i.member.as_ref(),
            Ix::Modal(i) => i.member.as_ref(),
        }
    }

    /// Permissions Discord computed for the invoker in this channel.
    pub fn permissions(&self) -> Permissions {
        self.member().and_then(|m| m.permissions).unwrap_or(Permissions::empty())
    }

    pub fn component_message(&self) -> Option<&Message> {
        match &self.ix {
            Ix::Comp(i) => Some(&i.message),
            _ => None,
        }
    }

    pub fn is_deferred(&self) -> bool {
        self.state.lock().unwrap().deferred
    }

    pub fn is_replied(&self) -> bool {
        self.state.lock().unwrap().replied
    }

    fn message_builder(data: &ReplyData) -> CreateInteractionResponseMessage {
        let mut m = CreateInteractionResponseMessage::new().ephemeral(data.ephemeral);
        if let Some(c) = &data.content {
            m = m.content(c.clone());
        }
        if !data.embeds.is_empty() {
            m = m.embeds(data.embeds.clone());
        }
        if let Some(rows) = &data.components {
            m = m.components(rows.clone());
        }
        if let Some(a) = &data.allowed_mentions {
            m = m.allowed_mentions(a.clone());
        }
        for f in &data.files {
            m = m.add_file(f.clone());
        }
        m
    }

    async fn respond(&self, response: CreateInteractionResponse) -> BotResult<()> {
        match &self.ix {
            Ix::Cmd(i) => i.create_response(&self.http, response).await?,
            Ix::Comp(i) => i.create_response(&self.http, response).await?,
            Ix::Modal(i) => i.create_response(&self.http, response).await?,
        }
        Ok(())
    }

    /// `deferReply`.
    pub async fn defer(&self, ephemeral: bool) -> BotResult<()> {
        self.respond(CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new().ephemeral(ephemeral))).await?;
        self.state.lock().unwrap().deferred = true;
        Ok(())
    }

    /// `deferUpdate` (buttons): acknowledge without touching the message.
    pub async fn defer_update(&self) -> BotResult<()> {
        self.respond(CreateInteractionResponse::Acknowledge).await?;
        self.state.lock().unwrap().deferred = true;
        Ok(())
    }

    /// `interaction.update(...)` for components: replace the message the button sits on.
    pub async fn update(&self, data: ReplyData) -> BotResult<()> {
        self.respond(CreateInteractionResponse::UpdateMessage(Self::message_builder(&data))).await?;
        self.state.lock().unwrap().replied = true;
        Ok(())
    }

    pub async fn edit_reply(&self, data: ReplyData) -> BotResult<()> {
        let mut e = EditInteractionResponse::new();
        if let Some(c) = &data.content {
            e = e.content(c.clone());
        }
        if !data.embeds.is_empty() {
            e = e.embeds(data.embeds.clone());
        }
        if let Some(rows) = &data.components {
            e = e.components(rows.clone());
        }
        if let Some(a) = &data.allowed_mentions {
            e = e.allowed_mentions(a.clone());
        }
        for f in &data.files {
            e = e.new_attachment(f.clone());
        }
        match &self.ix {
            Ix::Cmd(i) => i.edit_response(&self.http, e).await?,
            Ix::Comp(i) => i.edit_response(&self.http, e).await?,
            Ix::Modal(i) => i.edit_response(&self.http, e).await?,
        };
        self.state.lock().unwrap().replied = true;
        Ok(())
    }

    pub async fn follow_up(&self, data: ReplyData) -> BotResult<()> {
        let mut f = CreateInteractionResponseFollowup::new().ephemeral(data.ephemeral);
        if let Some(c) = &data.content {
            f = f.content(c.clone());
        }
        if !data.embeds.is_empty() {
            f = f.embeds(data.embeds.clone());
        }
        if let Some(rows) = &data.components {
            f = f.components(rows.clone());
        }
        if let Some(a) = &data.allowed_mentions {
            f = f.allowed_mentions(a.clone());
        }
        for file in &data.files {
            f = f.add_file(file.clone());
        }
        match &self.ix {
            Ix::Cmd(i) => i.create_followup(&self.http, f).await?,
            Ix::Comp(i) => i.create_followup(&self.http, f).await?,
            Ix::Modal(i) => i.create_followup(&self.http, f).await?,
        };
        self.state.lock().unwrap().replied = true;
        Ok(())
    }

    /// `interaction.reply` that degrades gracefully: edits after a defer, follows up after a reply.
    pub async fn reply(&self, data: ReplyData) -> BotResult<()> {
        let (deferred, replied) = {
            let s = self.state.lock().unwrap();
            (s.deferred, s.replied)
        };
        if deferred && !replied {
            let mut d = data;
            d.ephemeral = false;
            return self.edit_reply(d).await;
        }
        if replied || deferred {
            return self.follow_up(data).await;
        }
        self.respond(CreateInteractionResponse::Message(Self::message_builder(&data))).await?;
        self.state.lock().unwrap().replied = true;
        Ok(())
    }

    /// `respondEphemeral` from the recruitment manager.
    pub async fn ephemeral(&self, content: impl Into<String>) -> BotResult<()> {
        self.reply(ReplyData::text(content).ephemeral()).await
    }
}
