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
    prelude::*,
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

struct SQLPool;

impl TypeMapKey for SQLPool {
    type Value = Pool<MySqlConnection>;
}

struct VoiceManager;

impl TypeMapKey for VoiceManager {
    type Value = Arc<Mutex<ClientVoiceManager>>;
}

#[group]
#[commands(play)]
struct General;

struct Sound {
    name: String,
    id: u32,
    src: Vec<u8>,
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
        .configure(|c| c.prefix("?"))
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

async fn search_for_sound(query: &str, db_pool: MySqlPool) -> Result<Sound, Box<dyn std::error::Error>> {

    if query.to_lowercase().starts_with("id:") {
        let id = query[3..].parse::<u32>()?;

        let sound = sqlx::query_as_unchecked!(
            Sound,
            "
SELECT id, name, src
    FROM sounds
    WHERE id = ?
    LIMIT 1
            ",
            id
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
    WHERE name = ?
    ORDER BY rand()
    LIMIT 1
            ",
            name
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

#[command]
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

            let sound = search_for_sound(search_term, pool).await?;

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

        None => {
            msg.channel_id.say(&ctx, "You are not in a voice chat!").await?;
        }
    }

    Ok(())
}
