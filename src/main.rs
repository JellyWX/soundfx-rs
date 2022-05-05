#[macro_use]
extern crate lazy_static;

mod cmds;
mod consts;
mod error;
mod event_handlers;
mod models;
mod utils;

use std::{env, sync::Arc};

use dashmap::DashMap;
use dotenv::dotenv;
use poise::serenity::{
    builder::CreateApplicationCommands,
    model::{
        gateway::{Activity, GatewayIntents},
        id::{GuildId, UserId},
    },
};
use songbird::SerenityInit;
use sqlx::{MySql, Pool};
use tokio::sync::RwLock;

use crate::{event_handlers::listener, models::guild_data::GuildData};

// Which database driver are we using?
type Database = MySql;

pub struct Data {
    database: Pool<Database>,
    http: reqwest::Client,
    guild_data_cache: DashMap<GuildId, Arc<RwLock<GuildData>>>,
    join_sound_cache: DashMap<UserId, Option<u32>>,
}

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

pub async fn register_application_commands(
    ctx: &poise::serenity::client::Context,
    framework: &poise::Framework<Data, Error>,
    guild_id: Option<GuildId>,
) -> Result<(), poise::serenity::Error> {
    let mut commands_builder = CreateApplicationCommands::default();
    let commands = &framework.options().commands;
    for command in commands {
        if let Some(slash_command) = command.create_as_slash_command() {
            commands_builder.add_application_command(slash_command);
        }
        if let Some(context_menu_command) = command.create_as_context_menu_command() {
            commands_builder.add_application_command(context_menu_command);
        }
    }
    let commands_builder = poise::serenity::json::Value::Array(commands_builder.0);

    if let Some(guild_id) = guild_id {
        ctx.http
            .create_guild_application_commands(guild_id.0, &commands_builder)
            .await?;
    } else {
        ctx.http
            .create_global_application_commands(&commands_builder)
            .await?;
    }

    Ok(())
}

// entry point
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    env_logger::init();

    dotenv()?;

    let discord_token = env::var("DISCORD_TOKEN").expect("Missing DISCORD_TOKEN from environment");

    let options = poise::FrameworkOptions {
        commands: vec![
            cmds::info::help(),
            cmds::info::info(),
            cmds::manage::change_public(),
            cmds::manage::upload_new_sound(),
            cmds::manage::download_file(),
            cmds::manage::delete_sound(),
            cmds::play::play(),
            cmds::play::queue_play(),
            cmds::play::loop_play(),
            cmds::play::soundboard(),
            poise::Command {
                subcommands: vec![
                    cmds::search::list_guild_sounds(),
                    cmds::search::list_user_sounds(),
                ],
                ..cmds::search::list_sounds()
            },
            cmds::search::show_random_sounds(),
            cmds::search::search_sounds(),
            cmds::stop::stop_playing(),
            cmds::stop::disconnect(),
            cmds::settings::change_volume(),
            poise::Command {
                subcommands: vec![
                    cmds::settings::disable_greet_sound(),
                    cmds::settings::enable_greet_sound(),
                    cmds::settings::set_greet_sound(),
                    cmds::settings::unset_greet_sound(),
                ],
                ..cmds::settings::greet_sound()
            },
        ],
        allowed_mentions: None,
        listener: |ctx, event, _framework, data| Box::pin(listener(ctx, event, data)),
        ..Default::default()
    };

    let database = Pool::connect(&env::var("DATABASE_URL").expect("No database URL provided"))
        .await
        .unwrap();

    poise::Framework::build()
        .token(discord_token)
        .user_data_setup(move |ctx, _bot, framework| {
            Box::pin(async move {
                ctx.set_activity(Activity::watching("for /play")).await;

                register_application_commands(
                    ctx,
                    framework,
                    env::var("DEBUG_GUILD")
                        .map(|inner| GuildId(inner.parse().expect("DEBUG_GUILD not valid")))
                        .ok(),
                )
                .await
                .unwrap();

                Ok(Data {
                    http: reqwest::Client::new(),
                    database,
                    guild_data_cache: Default::default(),
                    join_sound_cache: Default::default(),
                })
            })
        })
        .options(options)
        .client_settings(move |client_builder| client_builder.register_songbird())
        .intents(GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILDS)
        .run_autosharded()
        .await?;

    Ok(())
}
