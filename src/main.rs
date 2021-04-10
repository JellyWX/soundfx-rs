#[macro_use]
extern crate lazy_static;

extern crate reqwest;

mod error;
mod framework;
mod guild_data;
mod sound;

use guild_data::GuildData;
use sound::Sound;

use regex_command_attr::command;

use serenity::{
    async_trait,
    client::{bridge::gateway::GatewayIntents, Client, Context},
    framework::standard::{Args, CommandResult},
    http::Http,
    model::{
        channel::Message,
        guild::Guild,
        id::{ChannelId, GuildId, RoleId},
        voice::VoiceState,
    },
    prelude::*,
    utils::shard_id,
};

use songbird::{
    create_player,
    error::JoinResult,
    events::EventHandler as SongbirdEventHandler,
    ffmpeg,
    input::{cached::Memory, Input},
    Call, Event, EventContext, SerenityInit,
};

use sqlx::mysql::MySqlPool;

use dotenv::dotenv;

use crate::{framework::RegexFramework, guild_data::CtxGuildData};
use dashmap::DashMap;
use serenity::model::channel::Channel;
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

const THEME_COLOR: u32 = 0x00e0f3;

lazy_static! {
    static ref MAX_SOUNDS: u32 = env::var("MAX_SOUNDS").unwrap().parse::<u32>().unwrap();
    static ref PATREON_GUILD: u64 = env::var("PATREON_GUILD").unwrap().parse::<u64>().unwrap();
    static ref PATREON_ROLE: u64 = env::var("PATREON_ROLE").unwrap().parse::<u64>().unwrap();
    static ref AUTODISCONNECT_TIMER: u64 = env::var("AUTODISCONNECT_TIMER")
        .unwrap_or("300".to_string())
        .parse::<u64>()
        .unwrap();
}

// create event handler for bot
struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: bool) {
        if is_new {
            if let Ok(token) = env::var("DISCORDBOTS_TOKEN") {
                let shard_count = ctx.cache.shard_count().await;
                let current_shard_id = shard_id(guild.id.as_u64().to_owned(), shard_count);

                let guild_count = ctx
                    .cache
                    .guilds()
                    .await
                    .iter()
                    .filter(|g| shard_id(g.as_u64().to_owned(), shard_count) == current_shard_id)
                    .count() as u64;

                let mut hm = HashMap::new();
                hm.insert("server_count", guild_count);
                hm.insert("shard_id", current_shard_id);
                hm.insert("shard_count", shard_count);

                let client = ctx
                    .data
                    .read()
                    .await
                    .get::<ReqwestClient>()
                    .cloned()
                    .expect("Could not get ReqwestClient from data");

                let response = client
                    .post(
                        format!(
                            "https://top.gg/api/bots/{}/stats",
                            ctx.cache.current_user_id().await.as_u64()
                        )
                        .as_str(),
                    )
                    .header("Authorization", token)
                    .json(&hm)
                    .send()
                    .await;

                if let Err(res) = response {
                    println!("DiscordBots Response: {:?}", res);
                }
            }
        }
    }

    async fn voice_state_update(
        &self,
        ctx: Context,
        guild_id_opt: Option<GuildId>,
        old: Option<VoiceState>,
        new: VoiceState,
    ) {
        if let Some(past_state) = old {
            if let (Some(guild_id), None) = (guild_id_opt, new.channel_id) {
                if let Some(channel_id) = past_state.channel_id {
                    if let Some(Channel::Guild(channel)) = channel_id.to_channel_cached(&ctx).await
                    {
                        if channel.members(&ctx).await.map(|m| m.len()).unwrap_or(0) <= 1 {
                            let songbird = songbird::get(&ctx).await.unwrap();

                            let _ = songbird.remove(guild_id).await;
                        }
                    }
                }
            }
        } else if let (Some(guild_id), Some(user_channel)) = (guild_id_opt, new.channel_id) {
            if let Some(guild) = ctx.cache.guild(guild_id).await {
                let pool = ctx
                    .data
                    .read()
                    .await
                    .get::<MySQL>()
                    .cloned()
                    .expect("Could not get SQLPool from data");

                let guild_data_opt = ctx.guild_data(guild.id).await;

                if let Ok(guild_data) = guild_data_opt {
                    let volume;
                    let allowed_greets;

                    {
                        let read = guild_data.read().await;

                        volume = read.volume;
                        allowed_greets = read.allow_greets;
                    }

                    if allowed_greets {
                        let join_id_res = sqlx::query!(
                            "
SELECT join_sound_id
    FROM users
    WHERE user = ? AND join_sound_id IS NOT NULL
                                    ",
                            new.user_id.as_u64()
                        )
                        .fetch_one(&pool)
                        .await;

                        if let Ok(join_id_record) = join_id_res {
                            let join_id = join_id_record.join_sound_id;

                            let mut sound = sqlx::query_as_unchecked!(
                                Sound,
                                "
SELECT name, id, plays, public, server_id, uploader_id
    FROM sounds
    WHERE id = ?
                                        ",
                                join_id
                            )
                            .fetch_one(&pool)
                            .await
                            .unwrap();

                            let (handler, _) = join_channel(&ctx, guild, user_channel).await;

                            let _ = play_audio(
                                &mut sound,
                                volume,
                                &mut handler.lock().await,
                                pool,
                                false,
                            )
                            .await;
                        }
                    }
                }
            }
        }
    }
}

async fn play_audio(
    sound: &mut Sound,
    volume: u8,
    call_handler: &mut MutexGuard<'_, Call>,
    mysql_pool: MySqlPool,
    loop_: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    {
        let (track, track_handler) =
            create_player(sound.store_sound_source(mysql_pool.clone()).await?.into());

        let _ = track_handler.set_volume(volume as f32 / 100.0);

        if loop_ {
            let _ = track_handler.enable_loop();
        } else {
            let _ = track_handler.disable_loop();
        }

        call_handler.play(track);
    }

    sound.plays += 1;
    sound.commit(mysql_pool).await?;

    Ok(())
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
            {
                // set call to deafen
                let _ = call.lock().await.deafen(true).await;
            }

            (call, Ok(()))
        } else {
            let (call, res) = songbird.join(guild.id, channel_id).await;

            {
                // set call to deafen
                let _ = call.lock().await.deafen(true).await;
            }

            (call, res)
        }
    } else {
        let (call, res) = songbird.join(guild.id, channel_id).await;

        {
            // set call to deafen
            let _ = call.lock().await.deafen(true).await;
        }

        (call, res)
    };

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
        .add_command("help", &HELP_COMMAND)
        .add_command("info", &INFO_COMMAND)
        .add_command("invite", &INFO_COMMAND)
        .add_command("donate", &INFO_COMMAND)
        // play commands
        .add_command("loop", &LOOP_PLAY_COMMAND)
        .add_command("play", &PLAY_COMMAND)
        .add_command("p", &PLAY_COMMAND)
        .add_command("stop", &STOP_PLAYING_COMMAND)
        // sound management commands
        .add_command("upload", &UPLOAD_NEW_SOUND_COMMAND)
        .add_command("delete", &DELETE_SOUND_COMMAND)
        .add_command("list", &LIST_SOUNDS_COMMAND)
        .add_command("public", &CHANGE_PUBLIC_COMMAND)
        // setting commands
        .add_command("prefix", &CHANGE_PREFIX_COMMAND)
        .add_command("roles", &SET_ALLOWED_ROLES_COMMAND)
        .add_command("volume", &CHANGE_VOLUME_COMMAND)
        .add_command("allow_greet", &ALLOW_GREET_SOUNDS_COMMAND)
        .add_command("greet", &SET_GREET_SOUND_COMMAND)
        // search commands
        .add_command("search", &SEARCH_SOUNDS_COMMAND)
        .add_command("popular", &SHOW_POPULAR_SOUNDS_COMMAND)
        .add_command("random", &SHOW_RANDOM_SOUNDS_COMMAND);

    if audio_index.is_some() {
        framework = framework.add_command("ambience", &PLAY_AMBIENCE_COMMAND);
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

        let mut data = client.data.write().await;

        data.insert::<GuildDataCache>(guild_data_cache);
        data.insert::<MySQL>(mysql_pool);

        data.insert::<ReqwestClient>(Arc::new(reqwest::Client::new()));

        if let Some(audio_index) = audio_index {
            data.insert::<AudioIndex>(Arc::new(audio_index));
        }
    }

    client.start_autosharded().await?;

    Ok(())
}

#[command]
async fn help(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    if args.is_empty() {
        let description = {
            let guild_data = ctx.guild_data(msg.guild_id.unwrap()).await.unwrap();

            let read_lock = guild_data.read().await;

            format!(
                "Type `{}help category` to view help for a command category below:",
                read_lock.prefix
            )
        };

        msg.channel_id
            .send_message(&ctx, |m| {
                m.embed(|e| {
                    e.title("Help")
                        .color(THEME_COLOR)
                        .description(description)
                        .field("Info", "`help` `info` `invite` `donate`", false)
                        .field("Play", "`play` `p` `stop` `loop`", false)
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

        msg.channel_id
            .send_message(&ctx, |m| {
                m.embed(|e| e.title("Help").color(THEME_COLOR).description(body))
            })
            .await?;
    }

    Ok(())
}

#[command]
#[permission_level(Managed)]
async fn play(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    play_cmd(ctx, msg, args, false).await
}

#[command]
#[permission_level(Managed)]
async fn loop_play(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    play_cmd(ctx, msg, args, true).await
}

async fn play_cmd(ctx: &Context, msg: &Message, args: Args, loop_: bool) -> CommandResult {
    let guild = match msg.guild(&ctx.cache).await {
        Some(guild) => guild,

        None => {
            return Ok(());
        }
    };

    let guild_id = guild.id;

    let channel_to_join = guild
        .voice_states
        .get(&msg.author.id)
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

            let mut sound_vec = Sound::search_for_sound(
                search_term,
                *guild_id.as_u64(),
                *msg.author.id.as_u64(),
                pool.clone(),
                true,
            )
            .await?;

            let sound_res = sound_vec.first_mut();

            match sound_res {
                Some(sound) => {
                    {
                        let (call_handler, _) =
                            join_channel(ctx, guild.clone(), user_channel).await;

                        let guild_data = ctx.guild_data(guild).await.unwrap();

                        let mut lock = call_handler.lock().await;

                        play_audio(
                            sound,
                            guild_data.read().await.volume,
                            &mut lock,
                            pool,
                            loop_,
                        )
                        .await?;
                    }

                    msg.channel_id
                        .say(
                            &ctx,
                            format!("Playing sound {} with ID {}", sound.name, sound.id),
                        )
                        .await?;
                }

                None => {
                    msg.channel_id
                        .say(&ctx, "Couldn't find sound by term provided")
                        .await?;
                }
            }
        }

        None => {
            msg.channel_id
                .say(&ctx, "You are not in a voice chat!")
                .await?;
        }
    }

    Ok(())
}

struct RestartTrack;

#[async_trait]
impl SongbirdEventHandler for RestartTrack {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(&[(_state, track)]) = ctx {
            let _ = track.seek_time(Default::default());
        }

        None
    }
}

#[command]
#[permission_level(Managed)]
async fn play_ambience(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let guild = match msg.guild(&ctx.cache).await {
        Some(guild) => guild,

        None => {
            return Ok(());
        }
    };

    let channel_to_join = guild
        .voice_states
        .get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    match channel_to_join {
        Some(user_channel) => {
            let search_name = args.rest().to_lowercase();
            let audio_index = ctx.data.read().await.get::<AudioIndex>().cloned().unwrap();

            if let Some(filename) = audio_index.get(&search_name) {
                {
                    let (call_handler, _) = join_channel(ctx, guild.clone(), user_channel).await;

                    let guild_data = ctx.guild_data(guild).await.unwrap();

                    let mut lock = call_handler.lock().await;

                    // stop anything currently playing
                    lock.stop();

                    let (track, track_handler) = create_player(
                        Input::try_from(
                            Memory::new(ffmpeg(format!("audio/{}", filename)).await.unwrap())
                                .unwrap(),
                        )
                        .unwrap(),
                    );

                    let _ = track_handler.set_volume(guild_data.read().await.volume as f32 / 100.0);
                    let _ = track_handler.add_event(
                        Event::Periodic(
                            track_handler.metadata().duration.unwrap() - Duration::from_millis(500),
                            None,
                        ),
                        RestartTrack {},
                    );

                    lock.play(track);
                }

                msg.channel_id
                    .say(&ctx, format!("Playing ambience **{}**", search_name))
                    .await?;
            } else {
                msg.channel_id
                    .send_message(&ctx, |m| {
                        m.embed(|e| {
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
                        })
                    })
                    .await?;
            }
        }

        None => {
            msg.channel_id
                .say(&ctx, "You are not in a voice chat!")
                .await?;
        }
    }

    Ok(())
}

#[command]
#[permission_level(Managed)]
async fn stop_playing(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let voice_manager = songbird::get(ctx).await.unwrap();

    let _ = voice_manager.remove(msg.guild_id.unwrap()).await;

    Ok(())
}

#[command]
async fn info(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let current_user = ctx.cache.current_user().await;

    msg.channel_id.send_message(&ctx, |m| m
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
There is a maximum sound limit per user. This can be removed by donating at https://patreon.com/jellywx", current_user.name, current_user.id.as_u64())))).await?;

    Ok(())
}

#[command]
#[permission_level(Managed)]
async fn change_volume(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let guild_data_opt = ctx.guild_data(msg.guild_id.unwrap()).await;
    let guild_data = guild_data_opt.unwrap();

    if args.len() == 1 {
        match args.single::<u8>() {
            Ok(volume) => {
                guild_data.write().await.volume = volume;

                guild_data.read().await.commit(pool).await?;

                msg.channel_id
                    .say(&ctx, format!("Volume changed to {}%", volume))
                    .await?;
            }

            Err(_) => {
                let read = guild_data.read().await;

                msg.channel_id.say(&ctx,
                                   format!("Current server volume: {vol}%. Change the volume with ```{prefix}volume <new volume>```",
                                           vol = read.volume, prefix = read.prefix)).await?;
            }
        }
    } else {
        let read = guild_data.read().await;

        msg.channel_id.say(&ctx,
                           format!("Current server volume: {vol}%. Change the volume with ```{prefix}volume <new volume>```",
                                   vol = read.volume, prefix = read.prefix)).await?;
    }

    Ok(())
}

#[command]
#[permission_level(Restricted)]
async fn change_prefix(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let guild_data;

    {
        let guild_data_opt = ctx.guild_data(msg.guild_id.unwrap()).await;

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

                    msg.channel_id.say(&ctx, reply).await?;
                } else {
                    msg.channel_id
                        .say(&ctx, "Prefix must be less than 5 characters long")
                        .await?;
                }
            }

            Err(_) => {
                msg.channel_id
                    .say(
                        &ctx,
                        format!(
                            "Usage: `{prefix}prefix <new prefix>`",
                            prefix = guild_data.read().await.prefix
                        ),
                    )
                    .await?;
            }
        }
    } else {
        msg.channel_id
            .say(
                &ctx,
                format!(
                    "Usage: `{prefix}prefix <new prefix>`",
                    prefix = guild_data.read().await.prefix
                ),
            )
            .await?;
    }

    Ok(())
}

#[command]
async fn upload_new_sound(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
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

#[command]
#[permission_level(Restricted)]
async fn set_allowed_roles(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
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

#[command]
async fn list_sounds(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
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
        sounds = Sound::get_user_sounds(*msg.author.id.as_u64(), pool).await?;

        message_buffer = "All your sounds: ".to_string();
    } else {
        sounds = Sound::get_guild_sounds(*msg.guild_id.unwrap().as_u64(), pool).await?;

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
            msg.channel_id.say(&ctx, message_buffer).await?;

            message_buffer = "".to_string();
        }
    }

    if message_buffer.len() > 0 {
        msg.channel_id.say(&ctx, message_buffer).await?;
    }

    Ok(())
}

#[command]
async fn change_public(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");
    let uid = msg.author.id.as_u64();

    let name = args.rest();
    let gid = *msg.guild_id.unwrap().as_u64();

    let mut sound_vec = Sound::search_for_sound(name, gid, *uid, pool.clone(), true).await?;
    let sound_result = sound_vec.first_mut();

    match sound_result {
        Some(sound) => {
            if sound.uploader_id != Some(*uid) {
                msg.channel_id.say(&ctx, "You can only change the availability of sounds you have uploaded. Use `?list me` to view your sounds").await?;
            } else {
                if sound.public {
                    sound.public = false;

                    msg.channel_id
                        .say(&ctx, "Sound has been set to private 🔒")
                        .await?;
                } else {
                    sound.public = true;

                    msg.channel_id
                        .say(&ctx, "Sound has been set to public 🔓")
                        .await?;
                }

                sound.commit(pool).await?
            }
        }

        None => {
            msg.channel_id
                .say(&ctx, "Sound could not be found by that name.")
                .await?;
        }
    }

    Ok(())
}

#[command]
async fn delete_sound(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let uid = *msg.author.id.as_u64();
    let gid = *msg.guild_id.unwrap().as_u64();

    let name = args.rest();

    let sound_vec = Sound::search_for_sound(name, gid, uid, pool.clone(), true).await?;
    let sound_result = sound_vec.first();

    match sound_result {
        Some(sound) => {
            if sound.uploader_id != Some(uid) && sound.server_id != gid {
                msg.channel_id
                    .say(
                        &ctx,
                        "You can only delete sounds from this guild or that you have uploaded.",
                    )
                    .await?;
            } else {
                sound.delete(pool).await?;

                msg.channel_id.say(&ctx, "Sound has been deleted").await?;
            }
        }

        None => {
            msg.channel_id
                .say(&ctx, "Sound could not be found by that name.")
                .await?;
        }
    }

    Ok(())
}

async fn format_search_results(
    search_results: Vec<Sound>,
    msg: &Message,
    ctx: &Context,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

    msg.channel_id
        .send_message(&ctx, |m| m.embed(|e| e.title(title).fields(field_iter)))
        .await?;

    Ok(())
}

#[command]
async fn search_sounds(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
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
        *msg.guild_id.unwrap().as_u64(),
        *msg.author.id.as_u64(),
        pool,
        false,
    )
    .await?;

    format_search_results(search_results, msg, ctx).await?;

    Ok(())
}

#[command]
async fn show_popular_sounds(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
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
    ORDER BY plays DESC
    LIMIT 25
        "
    )
    .fetch_all(&pool)
    .await?;

    format_search_results(search_results, msg, ctx).await?;

    Ok(())
}

#[command]
async fn show_random_sounds(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
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
    ORDER BY rand()
    LIMIT 25
        "
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    format_search_results(search_results, msg, ctx)
        .await
        .unwrap();

    Ok(())
}

#[command]
async fn set_greet_sound(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let query = args.rest();
    let user_id = *msg.author.id.as_u64();

    let _ = sqlx::query!(
        "
INSERT IGNORE INTO users (user)
    VALUES (?)
        ",
        user_id
    )
    .execute(&pool)
    .await;

    if query.len() == 0 {
        sqlx::query!(
            "
UPDATE users
SET
    join_sound_id = NULL
WHERE
    user = ?
            ",
            user_id
        )
        .execute(&pool)
        .await?;

        msg.channel_id
            .say(&ctx, "Your greet sound has been unset.")
            .await?;
    } else {
        let sound_vec = Sound::search_for_sound(
            query,
            *msg.guild_id.unwrap().as_u64(),
            user_id,
            pool.clone(),
            true,
        )
        .await?;

        match sound_vec.first() {
            Some(sound) => {
                sound.set_as_greet(user_id, pool).await?;

                msg.channel_id
                    .say(
                        &ctx,
                        format!(
                            "Greet sound has been set to {} (ID {})",
                            sound.name, sound.id
                        ),
                    )
                    .await?;
            }

            None => {
                msg.channel_id
                    .say(&ctx, "Could not find a sound by that name.")
                    .await?;
            }
        }
    }

    Ok(())
}

#[command]
#[permission_level(Managed)]
async fn allow_greet_sounds(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not acquire SQL pool from data");

    let guild_data_opt = ctx.guild_data(msg.guild_id.unwrap()).await;

    if let Ok(guild_data) = guild_data_opt {
        let current = guild_data.read().await.allow_greets;

        {
            guild_data.write().await.allow_greets = !current;
        }

        guild_data.read().await.commit(pool).await?;

        msg.channel_id
            .say(
                &ctx,
                format!(
                    "Greet sounds have been {}abled in this server",
                    if !current { "en" } else { "dis" }
                ),
            )
            .await?;
    }

    Ok(())
}
