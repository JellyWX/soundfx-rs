use serenity::{
    client::{
        Client, Context
    },
    framework::standard::{
        Args, StandardFramework
    },
    model::{
        channel::Message
    },
    prelude::{
        EventHandler, TypeMapKey, Mutex
    }
};

use sqlx::{
    Pool,
    mysql::MySqlPool
};

use dotenv::dotenv;

use std::{env, sync::Arc};

struct SQLPool;

impl TypeMapKey for SQLPool {
    type Value = Pool<MySqlConnection>;
}

struct VoiceManager;

impl TypeMapKey for VoiceManager {
    type Value = Arc<Mutex<ClientVoiceManager>>;
}

#[group]
#[commands()]
struct General;

struct Sound {
    name: String,
    id: u32,
    src: Vec<u8>,
}

// create event handler for bot
struct Handler;

impl EventHandler for Handler {}

// entry point
#[tokio::main]
fn main() {
    dotenv();

    let framework = StandardFramework::new()
        .configure(|c| c.prefix("?"))
        .group(&GENERAL_GROUP);

    let mut client = Client::new_with_framework(&env::var("DISCORD_TOKEN").expect("Missing token from environment"), Handler, framework)
        .await
        .expect("Error occurred creating client");

    {
        let pool = MySqlPool::new(env::var("DATABASE_URL"));

        let mut data = client.data.write().await;
        data.insert::<SQLPool>(pool);

        data.insert::<VoiceManager>(Arc::clone(&client.voice_manager));
    }

    let _ = client.start().await.map_err(|reason| println!("Failed to start client: {:?}", reason));
}

async fn search_for_sound(query: String, db_connector: &MySqlConnection) -> Result<Sound, Box<dyn std::error::Error>> {

    if query.to_lowercase().starts_with("id:") {
        let id = query[3..].parse::<u32>()?;

        let sound = sqlx::query!(
            "
SELECT name, src
FROM sounds
WHERE id = ?
LIMIT 1
            ",
            id
        )
            .fetch_one(&db_connector)
            .await?;

        Ok(Sound {
            name: sound.name,
            id,
            src: sound.src,
        })
    }
    else {
        let name = query;

        let sound = sqlx::query!(
            "
SELECT id, src
FROM sounds
ORDER BY rand()
WHERE name = ?
LIMIT 1
            ",
            name
        )
            .fetch_one(&db_connector)
            .await?;

        Ok(Sound {
            name,
            id: sound.id,
            src: sound.src,
        })
    }
}


#[command]
async fn play(ctx: &mut Context, msg: &Message, args: Args) {
    let search_term = args.collect().join(" ");

    let pool_lock = ctx.data.read().await
        .get::<SQLPool>().expect("Could not get SQL Pool out of data");

    let mut pool = pool_lock.lock().await;

    let sound_res = search_for_sound(search_term, pool).await;

    match sound_res {
        Ok(sound) => {
            let source = sound.src;


        }

        Err(reason) => {

        }
    }
}
