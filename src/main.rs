#[macro_use]
extern crate lazy_static;

mod cmds;
mod error;
mod event_handlers;
mod guild_data;
mod sound;

use std::{env, sync::Arc};

use dashmap::DashMap;
use dotenv::dotenv;
use poise::serenity::{
    builder::CreateApplicationCommands,
    model::{
        channel::Channel,
        gateway::{Activity, GatewayIntents},
        guild::Guild,
        id::{ChannelId, GuildId, UserId},
    },
};
use songbird::{create_player, error::JoinResult, tracks::TrackHandle, Call, SerenityInit};
use sqlx::mysql::MySqlPool;
use tokio::sync::{Mutex, MutexGuard, RwLock};

use crate::{
    event_handlers::listener,
    guild_data::{CtxGuildData, GuildData},
    sound::Sound,
};

pub struct Data {
    database: MySqlPool,
    http: reqwest::Client,
    guild_data_cache: DashMap<GuildId, Arc<RwLock<GuildData>>>,
    join_sound_cache: DashMap<UserId, Option<u32>>,
}

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

const THEME_COLOR: u32 = 0x00e0f3;

lazy_static! {
    static ref MAX_SOUNDS: u32 = env::var("MAX_SOUNDS").unwrap().parse::<u32>().unwrap();
    static ref PATREON_GUILD: u64 = env::var("PATREON_GUILD").unwrap().parse::<u64>().unwrap();
    static ref PATREON_ROLE: u64 = env::var("PATREON_ROLE").unwrap().parse::<u64>().unwrap();
}

async fn play_audio(
    sound: &mut Sound,
    volume: u8,
    call_handler: &mut MutexGuard<'_, Call>,
    mysql_pool: MySqlPool,
    loop_: bool,
) -> Result<TrackHandle, Box<dyn std::error::Error + Send + Sync>> {
    let (track, track_handler) =
        create_player(sound.store_sound_source(mysql_pool.clone()).await?.into());

    let _ = track_handler.set_volume(volume as f32 / 100.0);

    if loop_ {
        let _ = track_handler.enable_loop();
    } else {
        let _ = track_handler.disable_loop();
    }

    call_handler.play(track);

    Ok(track_handler)
}

async fn join_channel(
    ctx: &poise::serenity_prelude::Context,
    guild: Guild,
    channel_id: ChannelId,
) -> (Arc<Mutex<Call>>, JoinResult<()>) {
    let songbird = songbird::get(ctx).await.unwrap();
    let current_user = ctx.cache.current_user_id();

    let current_voice_state = guild
        .voice_states
        .get(&current_user)
        .and_then(|voice_state| voice_state.channel_id);

    let (call, res) = if current_voice_state == Some(channel_id) {
        let call_opt = songbird.get(guild.id);

        if let Some(call) = call_opt {
            (call, Ok(()))
        } else {
            let (call, res) = songbird.join(guild.id, channel_id).await;

            (call, res)
        }
    } else {
        let (call, res) = songbird.join(guild.id, channel_id).await;

        (call, res)
    };

    {
        // set call to deafen
        let _ = call.lock().await.deafen(true).await;
    }

    if let Some(Channel::Guild(channel)) = channel_id.to_channel_cached(&ctx) {
        channel
            .edit_voice_state(&ctx, ctx.cache.current_user(), |v| v.suppress(false))
            .await;
    }

    (call, res)
}

async fn play_from_query(
    ctx: &Context<'_>,
    guild: Guild,
    user_id: UserId,
    query: &str,
    loop_: bool,
) -> String {
    let guild_id = guild.id;

    let channel_to_join = guild
        .voice_states
        .get(&user_id)
        .and_then(|voice_state| voice_state.channel_id);

    match channel_to_join {
        Some(user_channel) => {
            let pool = ctx.data().database.clone();

            let mut sound_vec =
                Sound::search_for_sound(query, guild_id, user_id, pool.clone(), true)
                    .await
                    .unwrap();

            let sound_res = sound_vec.first_mut();

            match sound_res {
                Some(sound) => {
                    {
                        let (call_handler, _) =
                            join_channel(ctx.discord(), guild.clone(), user_channel).await;

                        let guild_data = ctx.guild_data(guild_id).await.unwrap();

                        let mut lock = call_handler.lock().await;

                        play_audio(
                            sound,
                            guild_data.read().await.volume,
                            &mut lock,
                            pool,
                            loop_,
                        )
                        .await
                        .unwrap();
                    }

                    format!("Playing sound {} with ID {}", sound.name, sound.id)
                }

                None => "Couldn't find sound by term provided".to_string(),
            }
        }

        None => "You are not in a voice chat!".to_string(),
    }
}

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
            cmds::info::info(),
            cmds::manage::change_public(),
            cmds::manage::upload_new_sound(),
            cmds::manage::delete_sound(),
            cmds::play::play(),
            cmds::play::loop_play(),
            cmds::play::soundboard(),
        ],
        allowed_mentions: None,
        listener: |ctx, event, _framework, data| Box::pin(listener(ctx, event, data)),
        ..Default::default()
    };

    let database = MySqlPool::connect(&env::var("DATABASE_URL").expect("No database URL provided"))
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
        .client_settings(move |client_builder| {
            client_builder
                .intents(
                    GatewayIntents::GUILD_VOICE_STATES
                        | GatewayIntents::GUILD_MESSAGES
                        | GatewayIntents::GUILDS,
                )
                .register_songbird()
        })
        .run_autosharded()
        .await?;

    Ok(())
}
