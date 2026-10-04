//! `/sportscar` and `/garage` (car mastery cards).

use crate::app::App;
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use serenity::all::*;

fn card(title: &str, description: &str, file: &str) -> CreateEmbed {
    CreateEmbed::new().title(format!("**{title}**")).description(description).thumbnail(format!("attachment://{file}")).colour(Colour::new(0xff9900))
}

async fn images(app: &App) -> Vec<CreateAttachment> {
    let dir = app.dirs.assets.join("mastery");
    let mut files = Vec::new();
    for name in ["rev_surge.jpg", "overdrive.jpg", "mega_tank.jpg", "extra_part.jpg"] {
        if let Ok(a) = CreateAttachment::path(dir.join(name)).await {
            files.push(a);
        }
    }
    files
}

fn embeds() -> Vec<CreateEmbed> {
    vec![
        card("REV SURGE", "**Increased acceleration**", "rev_surge.jpg"),
        card("OVERDRIVE", "**Boosts top speed**", "overdrive.jpg"),
        card("MEGA TANK", "**Expands fuel tank size**", "mega_tank.jpg"),
        card("EXTRA PART", "**Unlocks 4th mastery slot**", "extra_part.jpg"),
    ]
}

pub async fn slash_sportscar(app: &App, r: &Responder) -> BotResult<()> {
    let mut data = ReplyData::text("**Sports Car Mastery**");
    data.embeds = embeds();
    data.files = images(app).await;
    r.reply(data).await
}

pub async fn slash_garage(_app: &App, r: &Responder) -> BotResult<()> {
    let menu = CreateSelectMenu::new(
        "select_car",
        CreateSelectMenuKind::String { options: vec![CreateSelectMenuOption::new("Sports Car", "sports_car").emoji(ReactionType::Unicode("\u{1f3ce}\u{fe0f}".into()))] },
    )
    .placeholder("Choose a vehicle");
    r.reply(
        ReplyData::text("**Welcome to Mastery Garage!** Choose a vehicle to view its mastery:")
            .components(vec![CreateActionRow::SelectMenu(menu)])
            .ephemeral(),
    )
    .await
}

/// The garage menu never had a handler in the old bot (choosing a car did nothing); it shows the cards now.
pub async fn handle_select(app: &App, r: &Responder, values: &[String]) -> bool {
    if !values.iter().any(|v| v == "sports_car") {
        return false;
    }
    let mut data = ReplyData::text("**Sports Car Mastery**");
    data.embeds = embeds();
    data.files = images(app).await;
    let _ = r.reply(data).await;
    true
}
