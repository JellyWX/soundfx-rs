#[macro_use]
extern crate lazy_static;

mod error;
mod event_handlers;
mod framework;
mod guild_data;
mod sound;

use crate::{
    event_handlers::{Handler, RestartTrack},
    framework::{CommandInvoke, CreateGenericResponse, RegexFramework},
    guild_data::{CtxGuildData, GuildData},
    sound::{JoinSoundCtx, Sound},
};

use log::info;

use regex_command_attr::command;

use serenity::{
    client::{bridge::gateway::GatewayIntents, Client, Context},
    framework::standard::{Args, CommandResult},
    http::Http,
    model::{
        guild::Guild,
        id::{ChannelId, GuildId, RoleId, UserId},
    },
    prelude::*,
};

use songbird::{
    create_player,
    error::JoinResult,
    ffmpeg,
    input::{cached::Memory, Input},
    tracks::TrackHandle,
    Call, Event, SerenityInit,
};

use sqlx::mysql::MySqlPool;

use dotenv::dotenv;

use dashmap::DashMap;

use std::{collections::HashMap, convert::TryFrom, env, sync::Arc, time::Duration};

use tokio::sync::{MutexGuard, RwLock};

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
    let current_user = ctx.cache.current_user_id().await;

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

    (call, res)
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
        .add_command(&HELP_COMMAND)
        .add_command(&INFO_COMMAND)
        .add_command(&INFO_COMMAND)
        .add_command(&INFO_COMMAND)
        // play commands
        .add_command(&LOOP_PLAY_COMMAND)
        .add_command(&PLAY_COMMAND)
        .add_command(&STOP_PLAYING_COMMAND)
        .add_command(&DISCONNECT_COMMAND)
        // sound management commands
        .add_command(&UPLOAD_NEW_SOUND_COMMAND)
        .add_command(&DELETE_SOUND_COMMAND)
        .add_command(&LIST_SOUNDS_COMMAND)
        .add_command(&CHANGE_PUBLIC_COMMAND)
        // setting commands
        .add_command(&CHANGE_PREFIX_COMMAND)
        .add_command(&SET_ALLOWED_ROLES_COMMAND)
        .add_command(&CHANGE_VOLUME_COMMAND)
        .add_command(&ALLOW_GREET_SOUNDS_COMMAND)
        .add_command(&SET_GREET_SOUND_COMMAND)
        // search commands
        .add_command(&SEARCH_SOUNDS_COMMAND)
        .add_command(&SHOW_POPULAR_SOUNDS_COMMAND)
        .add_command(&SHOW_RANDOM_SOUNDS_COMMAND);

    if audio_index.is_some() {
        framework = framework.add_command(&PLAY_AMBIENCE_COMMAND);
    }

    framework = framework.build();

    let mut client =
        Client::builder(&env::var("DISCORD_TOKEN").expect("Missing token from environment"))
            .intents(
                GatewayIntents::GUILD_VOICE_STATES
                    | GatewayIntents::GUILD_MESSAGES
                    | GatewayIntents::GUILDS,
            )
            .framework(framework)
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

#[command]
async fn help(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    if args.is_empty() {
        let description = {
            let guild_data = ctx.guild_data(invoke.guild_id().unwrap()).await.unwrap();

            let read_lock = guild_data.read().await;

            format!(
                "Type `{}help category` to view help for a command category below:",
                read_lock.prefix
            )
        };

        invoke
            .channel_id()
            .send_message(&ctx, |m| {
                m.embed(|e| {
                    e.title("Help")
                        .color(THEME_COLOR)
                        .description(description)
                        .field("Info", "`help` `info` `invite` `donate`", false)
                        .field("Play", "`play` `p` `stop` `dc` `loop`", false)
                        .field("Manage", "`upload` `delete` `list` `public`", false)
                        .field("Settings", "`prefix` `roles` `volume` `allow_greet`", false)
                        .field("Search", "`search` `random` `popular`", false)
                        .field("Other", "`greet` `ambience`", false)
                })
            })
            .await?;
    } else {
        let body = match args.rest().to_lowercase().as_str() {
            "info" => {
                "__Info Commands__
`help` - view all commands
`help [category]` - view help for the commands in a category

`info` - view information about the bot

`invite` - get an invite link for the bot

`donate` - view information about the Patreon
                "
            }
            "play" => {
                "__Play Commands__
`play [sound]` - play a sound matching the name \"sound\"
`play [id]` - play the sound with numerical ID `id`

`p` - an alias for `play`

`stop` - stop the bot from playing
`dc` - disconnect the bot from the current channel

`loop [sound]` - play a sound matching the name \"sound\" on loop
`loop [id]` - play a sound matching the numerical ID `id` on loop
                "
            }
            "manage" => {
                "__Manage Commands__
`upload [name]` - upload a new sound effect to the name \"name\"

`delete [name]` - delete a sound you have uploaded under the name \"name\"

`list` - list sounds uploaded on the server you are on
`list me` - list sounds you have uploaded to any server

`public [name]` - make a sound you have uploaded public or private
                "
            }
            "settings" => {
                "__Settings Commands__
`prefix [new prefix]` - change the prefix of the bot

`roles [role list]` - set which roles can use the bot
`roles off` - allow all users to use the bot

`volume [new volume]` - change the volume of the bot

`allow_greet` - toggle whether users in your server can use greet sounds
                "
            }
            "search" => {
                "__Search Commands__
`search [term]` - search for sounds matching \"term\"

`random` - find some random sounds on the bot

`popular` - find the most played sounds on the bot
                "
            }
            "other" => {
                "__Other Commands__
`greet [name]` - set your greet sound (join sound) to the sound called \"name\"
`greet [id]` - set your greet sound (join sound) to the sound with numerical ID `id`

`ambience` - view a list of ambience sounds
`ambience [name]` - set an ambience sound playing
                "
            }
            _ => {
                "__Unrecognised Category__
Please select a category from the following:

`info`
`play`
`manage`
`settings`
`search`
`other`
                "
            }
        };

        invoke
            .channel_id()
            .send_message(&ctx, |m| {
                m.embed(|e| e.title("Help").color(THEME_COLOR).description(body))
            })
            .await?;
    }

    Ok(())
}

#[command]
#[aliases("p")]
#[required_permissions(Managed)]
async fn play(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let guild = invoke.guild(ctx.cache.clone()).await.unwrap();

    invoke
        .channel_id()
        .say(
            &ctx,
            play_cmd(ctx, guild, invoke.author_id(), args, false).await,
        )
        .await?;

    Ok(())
}

#[command]
#[required_permissions(Managed)]
async fn loop_play(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let guild = invoke.guild(ctx.cache.clone()).await.unwrap();

    invoke
        .respond(
            ctx.http.clone(),
            CreateGenericResponse::new()
                .content(play_cmd(ctx, guild, invoke.author_id(), args, true).await),
        )
        .await?;

    Ok(())
}

async fn play_cmd(ctx: &Context, guild: Guild, user_id: UserId, args: Args, loop_: bool) -> String {
    let guild_id = guild.id;

    let channel_to_join = guild
        .voice_states
        .get(&user_id)
        .and_then(|voice_state| voice_state.channel_id);

    match channel_to_join {
        Some(user_channel) => {
            let search_term = args.rest();

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

#[command("ambience")]
#[required_permissions(Managed)]
async fn play_ambience(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let guild = invoke.guild(ctx.cache.clone()).await.unwrap();

    let channel_to_join = guild
        .voice_states
        .get(&invoke.author_id())
        .and_then(|voice_state| voice_state.channel_id);

    match channel_to_join {
        Some(user_channel) => {
            let search_name = args.rest().to_lowercase();
            let audio_index = ctx.data.read().await.get::<AudioIndex>().cloned().unwrap();

            if let Some(filename) = audio_index.get(&search_name) {
                let (track, track_handler) = create_player(
                    Input::try_from(
                        Memory::new(ffmpeg(format!("audio/{}", filename)).await.unwrap()).unwrap(),
                    )
                    .unwrap(),
                );

                let (call_handler, _) = join_channel(ctx, guild.clone(), user_channel).await;
                let guild_data = ctx.guild_data(guild).await.unwrap();

                {
                    let mut lock = call_handler.lock().await;

                    lock.play(track);
                }

                let _ = track_handler.set_volume(guild_data.read().await.volume as f32 / 100.0);
                let _ = track_handler.add_event(
                    Event::Periodic(
                        track_handler.metadata().duration.unwrap() - Duration::from_millis(200),
                        None,
                    ),
                    RestartTrack {},
                );

                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new()
                            .content(format!("Playing ambience **{}**", search_name)),
                    )
                    .await?;
            } else {
                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new().embed(|e| {
                            e.title("Not Found").description(format!(
                                "Could not find ambience sound by name **{}**

__Available ambience sounds:__
{}",
                                search_name,
                                audio_index
                                    .keys()
                                    .into_iter()
                                    .map(|i| i.as_str())
                                    .collect::<Vec<&str>>()
                                    .join("\n")
                            ))
                        }),
                    )
                    .await?;
            }
        }

        None => {
            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().content("You are not in a voice chat!"),
                )
                .await?;
        }
    }

    Ok(())
}

#[command("stop")]
#[required_permissions(Managed)]
async fn stop_playing(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    _args: Args,
) -> CommandResult {
    let guild_id = invoke.guild_id().unwrap();

    let songbird = songbird::get(ctx).await.unwrap();
    let call_opt = songbird.get(guild_id);

    if let Some(call) = call_opt {
        let mut lock = call.lock().await;

        lock.stop();
    }

    Ok(())
}

#[command]
#[aliases("dc")]
#[required_permissions(Managed)]
async fn disconnect(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    _args: Args,
) -> CommandResult {
    let guild_id = invoke.guild_id().unwrap();

    let songbird = songbird::get(ctx).await.unwrap();
    let _ = songbird.leave(guild_id).await;

    Ok(())
}

#[command]
#[aliases("invite")]
async fn info(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    _args: Args,
) -> CommandResult {
    let current_user = ctx.cache.current_user().await;

    invoke.channel_id().send_message(&ctx, |m| m
        .embed(|e| e
            .title("Info")
            .color(THEME_COLOR)
            .footer(|f| f
                .text(concat!(env!("CARGO_PKG_NAME"), " ver ", env!("CARGO_PKG_VERSION"))))
            .description(format!("Default prefix: `?`

Reset prefix: `@{0} prefix ?`

Invite me: https://discordapp.com/oauth2/authorize?client_id={1}&scope=bot&permissions=36703232

**Welcome to SoundFX!**
Developer: <@203532103185465344>
Find me on https://discord.jellywx.com/ and on https://github.com/JellyWX :)

**Sound Credits**
\"The rain falls against the parasol\" https://freesound.org/people/straget/
\"Heavy Rain\" https://freesound.org/people/lebaston100/
\"Rain on Windows, Interior, A\" https://freesound.org/people/InspectorJ/
\"Seaside Waves, Close, A\" https://freesound.org/people/InspectorJ/
\"Small River 1 - Fast - Close\" https://freesound.org/people/Pfannkuchn/

**An online dashboard is available!** Visit https://soundfx.jellywx.com/dashboard
There is a maximum sound limit per user. This can be removed by subscribing at **https://patreon.com/jellywx**", current_user.name, current_user.id.as_u64())))).await?;

    Ok(())
}

#[command("volume")]
#[required_permissions(Managed)]
async fn change_volume(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    mut args: Args,
) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let guild_data_opt = ctx.guild_data(invoke.guild_id().unwrap()).await;
    let guild_data = guild_data_opt.unwrap();

    if args.len() == 1 {
        match args.single::<u8>() {
            Ok(volume) => {
                guild_data.write().await.volume = volume;

                guild_data.read().await.commit(pool).await?;

                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new()
                            .content(format!("Volume changed to {}%", volume)),
                    )
                    .await?;
            }

            Err(_) => {
                let read = guild_data.read().await;

                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new().content(format!(
                            "Current server volume: {vol}%. Change the volume with `/volume <new volume>`",
                            vol = read.volume
                        )),
                    )
                    .await?;
            }
        }
    } else {
        let read = guild_data.read().await;

        invoke
            .respond(
                ctx.http.clone(),
                CreateGenericResponse::new().content(format!(
                    "Current server volume: {vol}%. Change the volume with `/volume <new volume>`",
                    vol = read.volume
                )),
            )
            .await?;
    }

    Ok(())
}

#[command("prefix")]
#[required_permissions(Restricted)]
async fn change_prefix(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    mut args: Args,
) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let guild_data;

    {
        let guild_data_opt = ctx.guild_data(invoke.guild_id().unwrap()).await;

        guild_data = guild_data_opt.unwrap();
    }

    if args.len() == 1 {
        match args.single::<String>() {
            Ok(prefix) => {
                if prefix.len() <= 5 {
                    let reply = format!("Prefix changed to `{}`", prefix);

                    {
                        guild_data.write().await.prefix = prefix;
                    }

                    {
                        let read = guild_data.read().await;

                        read.commit(pool).await?;
                    }

                    invoke
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new().content(reply),
                        )
                        .await?;
                } else {
                    invoke
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new()
                                .content("Prefix must be less than 5 characters long"),
                        )
                        .await?;
                }
            }

            Err(_) => {
                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new().content(format!(
                            "Usage: `{prefix}prefix <new prefix>`",
                            prefix = guild_data.read().await.prefix
                        )),
                    )
                    .await?;
            }
        }
    } else {
        invoke
            .respond(
                ctx.http.clone(),
                CreateGenericResponse::new().content(format!(
                    "Usage: `{prefix}prefix <new prefix>`",
                    prefix = guild_data.read().await.prefix
                )),
            )
            .await?;
    }

    Ok(())
}

#[command("upload")]
#[allow_slash(false)]
async fn upload_new_sound(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let msg = invoke.msg().unwrap();

    fn is_numeric(s: &String) -> bool {
        for char in s.chars() {
            if char.is_digit(10) {
                continue;
            } else {
                return false;
            }
        }
        true
    }

    let new_name = args.rest().to_string();

    if !new_name.is_empty() && new_name.len() <= 20 {
        if !is_numeric(&new_name) {
            let pool = ctx
                .data
                .read()
                .await
                .get::<MySQL>()
                .cloned()
                .expect("Could not get SQLPool from data");

            // need to check the name is not currently in use by the user
            let count_name =
                Sound::count_named_user_sounds(*msg.author.id.as_u64(), &new_name, pool.clone())
                    .await?;
            if count_name > 0 {
                msg.channel_id.say(&ctx, "You are already using that name. Please choose a unique name for your upload.").await?;
            } else {
                // need to check how many sounds user currently has
                let count = Sound::count_user_sounds(*msg.author.id.as_u64(), pool.clone()).await?;
                let mut permit_upload = true;

                // need to check if user is patreon or nah
                if count >= *MAX_SOUNDS {
                    let patreon_guild_member =
                        GuildId(*PATREON_GUILD).member(ctx, msg.author.id).await;

                    if let Ok(member) = patreon_guild_member {
                        permit_upload = member.roles.contains(&RoleId(*PATREON_ROLE));
                    } else {
                        permit_upload = false;
                    }
                }

                if permit_upload {
                    let attachment = if let Some(attachment) = msg.attachments.get(0) {
                        Some(attachment.url.clone())
                    } else {
                        msg.channel_id.say(&ctx, "Please now upload an audio file under 1MB in size (larger files will be automatically trimmed):").await?;

                        let reply = msg
                            .channel_id
                            .await_reply(&ctx)
                            .author_id(msg.author.id)
                            .timeout(Duration::from_secs(30))
                            .await;

                        match reply {
                            Some(reply_msg) => {
                                if let Some(attachment) = reply_msg.attachments.get(0) {
                                    Some(attachment.url.clone())
                                } else {
                                    msg.channel_id.say(&ctx, "Please upload 1 attachment following your upload command. Aborted").await?;

                                    None
                                }
                            }

                            None => {
                                msg.channel_id
                                    .say(&ctx, "Upload timed out. Please redo the command")
                                    .await?;

                                None
                            }
                        }
                    };

                    if let Some(url) = attachment {
                        match Sound::create_anon(
                            &new_name,
                            url.as_str(),
                            *msg.guild_id.unwrap().as_u64(),
                            *msg.author.id.as_u64(),
                            pool,
                        )
                        .await
                        {
                            Ok(_) => {
                                msg.channel_id.say(&ctx, "Sound has been uploaded").await?;
                            }

                            Err(e) => {
                                println!("Error occurred during upload: {:?}", e);
                                msg.channel_id.say(&ctx, "Sound failed to upload.").await?;
                            }
                        }
                    }
                } else {
                    msg.channel_id.say(
                        &ctx,
                        format!(
                            "You have reached the maximum number of sounds ({}). Either delete some with `?delete` or join our Patreon for unlimited uploads at **https://patreon.com/jellywx**",
                            *MAX_SOUNDS,
                        )).await?;
                }
            }
        } else {
            msg.channel_id
                .say(
                    &ctx,
                    "Please ensure the sound name contains a non-numerical character",
                )
                .await?;
        }
    } else {
        msg.channel_id.say(&ctx, "Usage: `?upload <name>`. Please ensure the name provided is less than 20 characters in length").await?;
    }

    Ok(())
}

#[command("roles")]
#[required_permissions(Restricted)]
#[allow_slash(false)]
async fn set_allowed_roles(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let msg = invoke.msg().unwrap();
    let guild_id = *msg.guild_id.unwrap().as_u64();

    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    if args.len() == 0 {
        let roles = sqlx::query!(
            "
SELECT role
    FROM roles
    WHERE guild_id = ?
            ",
            guild_id
        )
        .fetch_all(&pool)
        .await?;

        let all_roles = roles
            .iter()
            .map(|i| format!("<@&{}>", i.role.to_string()))
            .collect::<Vec<String>>()
            .join(", ");

        msg.channel_id.say(&ctx, format!("Usage: `?roles <role mentions or anything else to disable>`. Current roles: {}", all_roles)).await?;
    } else {
        sqlx::query!(
            "
DELETE FROM roles
    WHERE guild_id = ?
            ",
            guild_id
        )
        .execute(&pool)
        .await?;

        if msg.mention_roles.len() > 0 {
            for role in msg.mention_roles.iter().map(|r| *r.as_u64()) {
                sqlx::query!(
                    "
INSERT INTO roles (guild_id, role)
    VALUES
        (?, ?)
                    ",
                    guild_id,
                    role
                )
                .execute(&pool)
                .await?;
            }

            msg.channel_id
                .say(&ctx, "Specified roles whitelisted")
                .await?;
        } else {
            sqlx::query!(
                "
INSERT INTO roles (guild_id, role)
    VALUES
        (?, ?)
                    ",
                guild_id,
                guild_id
            )
            .execute(&pool)
            .await?;

            msg.channel_id
                .say(&ctx, "Role whitelisting disabled")
                .await?;
        }
    }

    Ok(())
}

#[command("list")]
async fn list_sounds(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let sounds;
    let mut message_buffer;

    if args.rest() == "me" {
        sounds = Sound::get_user_sounds(invoke.author_id(), pool).await?;

        message_buffer = "All your sounds: ".to_string();
    } else {
        sounds = Sound::get_guild_sounds(invoke.guild_id().unwrap(), pool).await?;

        message_buffer = "All sounds on this server: ".to_string();
    }

    for sound in sounds {
        message_buffer.push_str(
            format!(
                "**{}** ({}), ",
                sound.name,
                if sound.public { "🔓" } else { "🔒" }
            )
            .as_str(),
        );

        if message_buffer.len() > 2000 {
            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().content(message_buffer),
                )
                .await?;

            message_buffer = "".to_string();
        }
    }

    if message_buffer.len() > 0 {
        invoke
            .respond(
                ctx.http.clone(),
                CreateGenericResponse::new().content(message_buffer),
            )
            .await?;
    }

    Ok(())
}

#[command("public")]
async fn change_public(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let uid = invoke.author_id().as_u64().to_owned();

    let name = args.rest();
    let gid = *invoke.guild_id().unwrap().as_u64();

    let mut sound_vec = Sound::search_for_sound(name, gid, uid, pool.clone(), true).await?;
    let sound_result = sound_vec.first_mut();

    match sound_result {
        Some(sound) => {
            if sound.uploader_id != Some(uid) {
                invoke.respond(ctx.http.clone(), CreateGenericResponse::new().content("You can only change the visibility of sounds you have uploaded. Use `?list me` to view your sounds")).await?;
            } else {
                if sound.public {
                    sound.public = false;

                    invoke
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new()
                                .content("Sound has been set to private 🔒"),
                        )
                        .await?;
                } else {
                    sound.public = true;

                    invoke
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new().content("Sound has been set to public 🔓"),
                        )
                        .await?;
                }

                sound.commit(pool).await?
            }
        }

        None => {
            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().content("Sound could not be found by that name."),
                )
                .await?;
        }
    }

    Ok(())
}

#[command("delete")]
async fn delete_sound(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let uid = invoke.author_id().0;
    let gid = invoke.guild_id().unwrap().0;

    let name = args.rest();

    let sound_vec = Sound::search_for_sound(name, gid, uid, pool.clone(), true).await?;
    let sound_result = sound_vec.first();

    match sound_result {
        Some(sound) => {
            if sound.uploader_id != Some(uid) && sound.server_id != gid {
                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new().content(
                            "You can only delete sounds from this guild or that you have uploaded.",
                        ),
                    )
                    .await?;
            } else {
                let has_perms = {
                    if let Ok(member) = invoke.member(&ctx).await {
                        if let Ok(perms) = member.permissions(&ctx).await {
                            perms.manage_guild()
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                if sound.uploader_id == Some(uid) || has_perms {
                    sound.delete(pool).await?;

                    invoke
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new().content("Sound has been deleted"),
                        )
                        .await?;
                } else {
                    invoke
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new().content(
                                "Only server admins can delete sounds uploaded by other users.",
                            ),
                        )
                        .await?;
                }
            }
        }

        None => {
            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().content("Sound could not be found by that name."),
                )
                .await?;
        }
    }

    Ok(())
}

fn format_search_results(search_results: Vec<Sound>) -> CreateGenericResponse {
    let mut current_character_count = 0;
    let title = "Public sounds matching filter:";

    let field_iter = search_results
        .iter()
        .take(25)
        .map(|item| {
            (
                &item.name,
                format!("ID: {}\nPlays: {}", item.id, item.plays),
                true,
            )
        })
        .filter(|item| {
            current_character_count += item.0.len() + item.1.len();

            current_character_count <= 2048 - title.len()
        });

    CreateGenericResponse::new().embed(|e| e.title(title).fields(field_iter))
}

#[command("search")]
async fn search_sounds(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let query = args.rest();

    let search_results = Sound::search_for_sound(
        query,
        invoke.guild_id().unwrap(),
        invoke.author_id(),
        pool,
        false,
    )
    .await?;

    invoke
        .respond(ctx.http.clone(), format_search_results(search_results))
        .await?;

    Ok(())
}

#[command("popular")]
async fn show_popular_sounds(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    _args: Args,
) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let search_results = sqlx::query_as_unchecked!(
        Sound,
        "
SELECT name, id, plays, public, server_id, uploader_id
    FROM sounds
    WHERE public = 1
    ORDER BY plays DESC
    LIMIT 25
        "
    )
    .fetch_all(&pool)
    .await?;

    invoke
        .respond(ctx.http.clone(), format_search_results(search_results))
        .await?;

    Ok(())
}

#[command("random")]
async fn show_random_sounds(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    _args: Args,
) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let search_results = sqlx::query_as_unchecked!(
        Sound,
        "
SELECT name, id, plays, public, server_id, uploader_id
    FROM sounds
    WHERE public = 1
    ORDER BY rand()
    LIMIT 25
        "
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    invoke
        .respond(ctx.http.clone(), format_search_results(search_results))
        .await?;

    Ok(())
}

#[command("greet")]
async fn set_greet_sound(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let query = args.rest();
    let user_id = invoke.author_id();

    if query.len() == 0 {
        ctx.update_join_sound(user_id, None).await;

        invoke
            .respond(
                ctx.http.clone(),
                CreateGenericResponse::new().content("Your greet sound has been unset."),
            )
            .await?;
    } else {
        let sound_vec = Sound::search_for_sound(
            query,
            invoke.guild_id().unwrap(),
            user_id,
            pool.clone(),
            true,
        )
        .await?;

        match sound_vec.first() {
            Some(sound) => {
                ctx.update_join_sound(user_id, Some(sound.id)).await;

                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new().content(format!(
                            "Greet sound has been set to {} (ID {})",
                            sound.name, sound.id
                        )),
                    )
                    .await?;
            }

            None => {
                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new()
                            .content("Could not find a sound by that name."),
                    )
                    .await?;
            }
        }
    }

    Ok(())
}

#[command("allow_greet")]
#[required_permissions(Managed)]
async fn allow_greet_sounds(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    _args: Args,
) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not acquire SQL pool from data");

    let guild_data_opt = ctx.guild_data(invoke.guild_id().unwrap()).await;

    if let Ok(guild_data) = guild_data_opt {
        let current = guild_data.read().await.allow_greets;

        {
            guild_data.write().await.allow_greets = !current;
        }

        guild_data.read().await.commit(pool).await?;

        invoke
            .respond(
                ctx.http.clone(),
                CreateGenericResponse::new().content(format!(
                    "Greet sounds have been {}abled in this server",
                    if !current { "en" } else { "dis" }
                )),
            )
            .await?;
    }

    Ok(())
}
