pub mod action;
pub mod anime;
pub mod anime_ui;
pub mod individual_actions;
pub mod instagram;
pub mod kc;
pub mod km;
pub mod ping;
pub mod recommend;
pub mod status;
pub mod watchlist;

use crate::discord::{AppData, Error};

/// Collects and returns all registered slash commands.
pub fn all() -> Vec<poise::Command<AppData, Error>> {
    let mut cmds = vec![
        ping::ping(),
        status::status(),
        action::action(),
        kc::kc(),
        km::km(),
        anime::anime(),
        watchlist::watchlist(),
        recommend::recommend(),
        instagram::instagram(),
    ];
    cmds.extend(individual_actions::all_action_commands());
    cmds
}
