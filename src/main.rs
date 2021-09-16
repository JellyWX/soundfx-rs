#[macro_use]
extern crate lazy_static;

mod cmds;
mod error;
mod event_handlers;
mod framework;
mod guild_data;
mod sound;

use std::{collections::HashMap, env, sync::Arc};

use dashmap::DashMap;
use dotenv::dotenv;
use log::info;
use serenity::{
    client::{bridge::gateway::GatewayIntents, Client, Context},
    http::Http,
    model::{
        channel::Channel,
        guild::Guild,
        id::{ChannelId, GuildId, UserId},
    },
    prelude::{Mutex, TypeMapKey},
};
use songbird::{create_player, error::JoinResult, tracks::TrackHandle, Call, SerenityInit};
use sqlx::mysql::MySqlPool;
use tokio::sync::{MutexGuard, RwLock};

use crate::{
    event_handlers::Handler,
    framework::{Args, RegexFramework},
    guild_data::{CtxGuildData, GuildData},
    sound::Sound,
};

struct MySQL;

impl TypeMapKey for MySQL {
    type Value = MySqlPool;
}

struct ReqwestClient;

impl TypeMapKey for ReqwestClient {
    type Value = Arc<reqwest::Client>;
}

struct AudioIndex;

impl TypeMapKey for AudioIndex {
    type Value = Arc<HashMap<String, String>>;
}

struct GuildDataCache;

impl TypeMapKey for GuildDataCache {
    type Value = Arc<DashMap<GuildId, Arc<RwLock<GuildData>>>>;
}

struct JoinSoundCache;

impl TypeMapKey for JoinSoundCache {
    type Value = Arc<DashMap<UserId, Option<u32>>>;
}

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

    sound.plays += 1;
    sound.commit(mysql_pool).await?;

    Ok(track_handler)
}

async fn join_channel(
    ctx: &Context,
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
    ctx: &Context,
    guild: Guild,
    user_id: UserId,
    args: Args,
    loop_: bool,
) -> String {
    let guild_id = guild.id;

    let channel_to_join = guild
        .voice_states
        .get(&user_id)
        .and_then(|voice_state| voice_state.channel_id);

    match channel_to_join {
        Some(user_channel) => {
            let search_term = args.named("query").unwrap();

            let pool = ctx
                .data
                .read()
                .await
                .get::<MySQL>()
                .cloned()
                .expect("Could not get SQLPool from data");

            let mut sound_vec =
                Sound::search_for_sound(search_term, guild_id, user_id, pool.clone(), true)
                    .await
                    .unwrap();

            let sound_res = sound_vec.first_mut();

            match sound_res {
                Some(sound) => {
                    {
                        let (call_handler, _) =
                            join_channel(ctx, guild.clone(), user_channel).await;

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

// entry point
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    env_logger::init();

    dotenv()?;

    let token = env::var("DISCORD_TOKEN").expect("Missing DISCORD_TOKEN from environment");

    let http = Http::new_with_token(&token);

    let logged_in_id = http.get_current_user().await?.id;
    let application_id = http.get_current_application_info().await?.id;

    let audio_index = if let Ok(static_audio) = std::fs::read_to_string("audio/audio.json") {
        if let Ok(json) = serde_json::from_str::<HashMap<String, String>>(&static_audio) {
            Some(json)
        } else {
            println!(
                "Invalid `audio.json` file. Not loading static audio or providing ambience command"
            );

            None
        }
    } else {
        println!("No `audio.json` file. Not loading static audio or providing ambience command");

        None
    };

    let mut framework = RegexFramework::new(logged_in_id)
        .default_prefix("?")
        .case_insensitive(true)
        .ignore_bots(true)
        // info commands
        .add_command(&cmds::info::HELP_COMMAND)
        .add_command(&cmds::info::INFO_COMMAND)
        // play commands
        .add_command(&cmds::play::LOOP_PLAY_COMMAND)
        .add_command(&cmds::play::PLAY_COMMAND)
        .add_command(&cmds::play::SOUNDBOARD_COMMAND)
        .add_command(&cmds::stop::STOP_PLAYING_COMMAND)
        .add_command(&cmds::stop::DISCONNECT_COMMAND)
        // sound management commands
        .add_command(&cmds::manage::UPLOAD_NEW_SOUND_COMMAND)
        .add_command(&cmds::manage::DELETE_SOUND_COMMAND)
        .add_command(&cmds::manage::CHANGE_PUBLIC_COMMAND)
        // setting commands
        .add_command(&cmds::settings::CHANGE_PREFIX_COMMAND)
        .add_command(&cmds::settings::SET_ALLOWED_ROLES_COMMAND)
        .add_command(&cmds::settings::CHANGE_VOLUME_COMMAND)
        .add_command(&cmds::settings::ALLOW_GREET_SOUNDS_COMMAND)
        .add_command(&cmds::settings::SET_GREET_SOUND_COMMAND)
        // search commands
        .add_command(&cmds::search::LIST_SOUNDS_COMMAND)
        .add_command(&cmds::search::SEARCH_SOUNDS_COMMAND)
        .add_command(&cmds::search::SHOW_POPULAR_SOUNDS_COMMAND)
        .add_command(&cmds::search::SHOW_RANDOM_SOUNDS_COMMAND);

    if audio_index.is_some() {
        framework = framework.add_command(&cmds::play::PLAY_AMBIENCE_COMMAND);
    }

    framework = framework.build();

    let framework_arc = Arc::new(framework);

    let mut client =
        Client::builder(&env::var("DISCORD_TOKEN").expect("Missing token from environment"))
            .intents(
                GatewayIntents::GUILD_VOICE_STATES
                    | GatewayIntents::GUILD_MESSAGES
                    | GatewayIntents::GUILDS,
            )
            .framework_arc(framework_arc.clone())
            .application_id(application_id.0)
            .event_handler(Handler)
            .register_songbird()
            .await
            .expect("Error occurred creating client");

    {
        let mysql_pool =
            MySqlPool::connect(&env::var("DATABASE_URL").expect("No database URL provided"))
                .await
                .unwrap();

        let guild_data_cache = Arc::new(DashMap::new());
        let join_sound_cache = Arc::new(DashMap::new());
        let mut data = client.data.write().await;

        data.insert::<GuildDataCache>(guild_data_cache);
        data.insert::<JoinSoundCache>(join_sound_cache);
        data.insert::<MySQL>(mysql_pool);
        data.insert::<RegexFramework>(framework_arc.clone());
        data.insert::<ReqwestClient>(Arc::new(reqwest::Client::new()));

        if let Some(audio_index) = audio_index {
            data.insert::<AudioIndex>(Arc::new(audio_index));
        }
    }

    if let Ok((Some(lower), Some(upper))) = env::var("SHARD_RANGE").map(|sr| {
        let mut split = sr
            .split(',')
            .map(|val| val.parse::<u64>().expect("SHARD_RANGE not an integer"));

        (split.next(), split.next())
    }) {
        let total_shards = env::var("SHARD_COUNT")
            .map(|shard_count| shard_count.parse::<u64>().ok())
            .ok()
            .flatten()
            .expect("No SHARD_COUNT provided, but SHARD_RANGE was provided");

        assert!(
            lower < upper,
            "SHARD_RANGE lower limit is not less than the upper limit"
        );

        info!(
            "Starting client fragment with shards {}-{}/{}",
            lower, upper, total_shards
        );

        client
            .start_shard_range([lower, upper], total_shards)
            .await?;
    } else if let Ok(total_shards) = env::var("SHARD_COUNT").map(|shard_count| {
        shard_count
            .parse::<u64>()
            .expect("SHARD_COUNT not an integer")
    }) {
        info!("Starting client with {} shards", total_shards);

        client.start_shards(total_shards).await?;
    } else {
        info!("Starting client as autosharded");

        client.start_autosharded().await?;
    }

    Ok(())
}
