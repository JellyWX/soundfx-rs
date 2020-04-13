use serenity::{
    client::Client,
    framework::StandardFramework,
    prelude::{
        EventHandler, Context, TypeMapKey
    }
};

use sqlx::{
    Pool,
    mysql::MySqlPool
};

use dotenv::dotenv;

use std::env;

struct SQLPool;

impl TypeMapKey for SQLPool {
    type Value = Pool<MySqlConnection>;
}

#[group]
#[commands()]
struct Commands;

// create event handler for bot
struct Handler;

impl EventHandler for Handler {}

// entry point
fn main() {
    dotenv();

    let mut client = Client::new(&env::var("DISCORD_TOKEN").expect("Missing token from environment"), Handler).expect("Failed to create client");

    client.with_framework(StandardFramework::new()
        .configure(|c| c.prefix("?"))
        .group(&GENERAL_GROUP));

    {
        let mut data = client.data.write();

        let pool = MySqlPool::new(env::var("DATABASE_URL"));

        data.insert::<SQLPool>(pool);
    }

    client.start().expect("Failed to start client");
}

#[command]
fn play() {

}
