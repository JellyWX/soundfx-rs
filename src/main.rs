use serenity::{
    client::{
        bridge::{
            gateway::GatewayIntents,
            voice::ClientVoiceManager,
        },
        Client, Context,
    },
    framework::standard::{
        Args, CommandResult, StandardFramework,
        macros::{
            command, group,
        }
    },
    model::{
        channel::Message
    },
    prelude::{
        Mutex as SerenityMutex,
        *
    },
    voice::ffmpeg,
};

use sqlx::{
    Pool,
    mysql::{
        MySqlPool,
        MySqlConnection,
    }
};

use dotenv::dotenv;

use tokio::{
    fs::File,
};

use std::{
    env,
    path::Path,
    sync::Arc,
};
use serenity::model::guild::Guild;
use tokio::sync::RwLockReadGuard;

struct SQLPool;

impl TypeMapKey for SQLPool {
    type Value = Pool<MySqlConnection>;
}

struct VoiceManager;

impl TypeMapKey for VoiceManager {
    type Value = Arc<SerenityMutex<ClientVoiceManager>>;
}

static THEME_COLOR: u32 = 0x00e0f3;

#[group]
#[commands(play, info, help, change_volume, )]
struct General;

struct Sound {
    name: String,
    id: u32,
    src: Vec<u8>,
}

struct GuildData {
    id: u64,
    pub name: Option<String>,
    pub prefix: String,
    pub volume: u8,
}

impl GuildData {
    async fn get_from_id(guild_id: u64, db_pool: MySqlPool) -> Option<GuildData> {
        let guild = sqlx::query_as!(
            GuildData,
            "
SELECT *
FROM servers
WHERE id = ?
            ", guild_id
        )
            .fetch_one(&db_pool)
            .await;

        match guild {
            Ok(guild) => Some(guild),

            Err(_) => None,
        }
    }

    async fn create_from_guild(guild: RwLockReadGuard<'_, Guild>, db_pool: MySqlPool) -> Result<GuildData, Box<dyn std::error::Error>> {
        let guild_data = sqlx::query!(
            "
INSERT INTO servers (id, name)
VALUES (?, ?)
            ", guild.id.as_u64(), guild.name
        )
            .execute(&db_pool)
            .await?;

        Ok(GuildData {
            id: *guild.id.as_u64(),
            name: Some(guild.name.clone()),
            prefix: String::from("?"),
            volume: 100,
        })
    }

    async fn commit(&self, db_pool: MySqlPool) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query!(
            "
UPDATE servers
SET
    name = ?,
    prefix = ?,
    volume = ?
WHERE
    id = ?
            ",
            self.name, self.prefix, self.volume, self.id
        )
            .execute(&db_pool)
            .await?;

        Ok(())
    }
}

// create event handler for bot
struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {}

// entry point
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv()?;

    let framework = StandardFramework::new()
        .configure(|c| c.dynamic_prefix(|ctx, msg| Box::pin(async move {
            let pool = ctx.data.read().await
                .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

            match GuildData::get_from_id(*msg.guild_id.unwrap().as_u64(), pool).await {
                Some(guild) => Some(guild.prefix),

                None => Some(String::from("?"))
            }
        })))
        .group(&GENERAL_GROUP);

    let mut client = Client::new_with_extras(
        &env::var("DISCORD_TOKEN").expect("Missing token from environment"),
        |extras| { extras
            .framework(framework)
            .event_handler(Handler)
            .intents(GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILDS)
        }).await.expect("Error occurred creating client");

    {
        let pool = MySqlPool::new(&env::var("DATABASE_URL").expect("No database URL provided")).await.unwrap();

        let mut data = client.data.write().await;
        data.insert::<SQLPool>(pool);

        data.insert::<VoiceManager>(Arc::clone(&client.voice_manager));
    }

    client.start().await?;

    Ok(())
}

async fn search_for_sound(query: &str, guild_id: u64, user_id: u64, db_pool: MySqlPool) -> Result<Sound, sqlx::Error> {

    fn extract_id(s: &str) -> Option<u32> {
        if s.to_lowercase().starts_with("id:") {
            match s[3..].parse::<u32>() {
                Ok(id) => Some(id),

                Err(_) => None
            }
        }
        else {
            None
        }
    }

    if let Some(id) = extract_id(&query[3..]) {
        let sound = sqlx::query_as_unchecked!(
            Sound,
            "
SELECT id, name, src
    FROM sounds
    WHERE id = ? AND (
        public = 1 OR
        uploader_id = ? OR
        server_id = ?
    )
    LIMIT 1
            ",
            id, user_id, guild_id
        )
            .fetch_one(&db_pool)
            .await?;

        Ok(sound)
    }
    else {
        let name = query;

        let sound = sqlx::query_as_unchecked!(
            Sound,
            "
SELECT id, name, src
    FROM sounds
    WHERE name = ? AND (
        public = 1 OR
        uploader_id = ? OR
        server_id = ?
    )
    ORDER BY rand(), public = 1, server_id = ?, uploader_id = ?
    LIMIT 1
            ",
            name, user_id, guild_id, guild_id, user_id
        )
            .fetch_one(&db_pool)
            .await?;

        Ok(sound)
    }
}

async fn store_sound_source(sound: &Sound) -> Result<String, Box<dyn std::error::Error>> {
    let caching_location = env::var("CACHING_LOCATION").unwrap_or(String::from("/tmp"));

    let path_name = format!("{}/sound-{}", caching_location, sound.id);
    let path = Path::new(&path_name);

    if !path.exists() {
        use tokio::prelude::*;

        let mut file = File::create(&path).await?;

        file.write_all(sound.src.as_ref()).await?;
    }

    Ok(path_name)
}

#[command("play")]
#[aliases("p")]
async fn play(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let guild = match msg.guild(&ctx.cache).await {
        Some(guild) => guild,

        None => {
            return Ok(());
        }
    };

    let guild_id = guild.read().await.id;

    let channel_to_join = guild.read().await
        .voice_states.get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    match channel_to_join {
        Some(user_channel) => {
            let search_term = args.rest();

            let pool = ctx.data.read().await
                .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

            let sound_res = search_for_sound(
                search_term,
                *guild_id.as_u64(),
                *msg.author.id.as_u64(),
                pool).await;

            match sound_res {
                Ok(sound) => {
                    let fp = store_sound_source(&sound).await?;

                    let voice_manager_lock = ctx.data.read().await
                        .get::<VoiceManager>().cloned().expect("Could not get VoiceManager from data");

                    let mut voice_manager = voice_manager_lock.lock().await;

                    match voice_manager.get_mut(guild_id) {
                        Some(handler) => {
                            // play sound
                            handler.play(ffmpeg(fp).await?);
                        }

                        None => {
                            // try & join a voice channel
                            match voice_manager.join(guild_id, user_channel) {
                                Some(handler) => {
                                    handler.play(ffmpeg(fp).await?);
                                }

                                None => {
                                    msg.channel_id.say(&ctx, "Failed to join channel").await?;
                                }
                            };
                        }
                    }
                }

                Err(_) => {
                    msg.channel_id.say(&ctx, "Couldn't find sound by term provided").await?;
                }
            }
        }

        None => {
            msg.channel_id.say(&ctx, "You are not in a voice chat!").await?;
        }
    }

    Ok(())
}

#[command]
async fn help(ctx: &mut Context, msg: &Message, _args: Args) -> CommandResult {
    msg.channel_id.send_message(&ctx, |m| m
        .embed(|e| e
            .title("Help")
            .color(THEME_COLOR)
            .description("Please visit our website at https://soundfx.jellywx.com/help"))).await?;

    Ok(())
}

#[command]
async fn info(ctx: &mut Context, msg: &Message, _args: Args) -> CommandResult {

    msg.channel_id.send_message(&ctx, |m| m
        .embed(|e| e
            .title("Info")
            .color(THEME_COLOR)
            .description("Default prefix: `?`

Reset prefix: `@SoundFX prefix ?`

Invite me: https://discordapp.com/oauth2/authorize?client_id=430384808200372245&scope=bot&permissions=36703232

**Welcome to SoundFX!**
Developer: <@203532103185465344>
Find me on https://discord.jellywx.com/ and on https://github.com/JellyWX :)

**An online dashboard is available!** Visit https://soundfx.jellywx.com/dashboard
There is a maximum sound limit per user. This can be removed by donating at https://patreon.com/jellywx"))).await?;

    Ok(())
}

#[command("volume")]
async fn change_volume(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let guild = match msg.guild(&ctx.cache).await {
        Some(guild) => guild,

        None => {
            return Ok(());
        }
    };

    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let mut guild_data_opt = GuildData::get_from_id(*guild.read().await.id.as_u64(), pool.clone()).await;

    if guild_data_opt.is_none() {
        guild_data_opt = Some(GuildData::create_from_guild(guild.read().await, pool.clone()).await.unwrap())
    }

    let mut guild_data = guild_data_opt.unwrap();

    if args.len() == 1 {
        match args.single::<u8>() {
            Ok(volume) => {
                guild_data.volume = volume;

                guild_data.commit(pool).await?;

                msg.channel_id.say(&ctx, format!("Volume changed to {}%", volume)).await?;
            }

            Err(_) => {
                msg.channel_id.say(&ctx,
                                   format!("Current server volume: {vol}%. Change the volume with ```{prefix}volume <new volume>```",
                                           vol = guild_data.volume, prefix = guild_data.prefix)).await?;
            }
        }
    }
    else {
        msg.channel_id.say(&ctx,
                           format!("Current server volume: {vol}%. Change the volume with ```{prefix}volume <new volume>```",
                                   vol = guild_data.volume, prefix = guild_data.prefix)).await?;
    }

    Ok(())
}
