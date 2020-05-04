#[macro_use]
extern crate lazy_static;

use serenity::{
    client::{
        bridge::{
            gateway::GatewayIntents,
            voice::ClientVoiceManager,
        },
        Client, Context,
    },
    framework::standard::{
        Args, CommandResult, CheckResult, StandardFramework, Reason,
        macros::{
            command, group, check,
        }
    },
    model::{
        id::{
            GuildId,
            RoleId,
        },
        channel::Message,
        guild::Guild,
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
    process::Command,
    sync::RwLockReadGuard,
};

use std::{
    env,
    path::Path,
    sync::Arc,
    time::Duration,
};
use std::fmt::Formatter;


struct SQLPool;

impl TypeMapKey for SQLPool {
    type Value = Pool<MySqlConnection>;
}

struct VoiceManager;

impl TypeMapKey for VoiceManager {
    type Value = Arc<SerenityMutex<ClientVoiceManager>>;
}

static THEME_COLOR: u32 = 0x00e0f3;

lazy_static! {
    static ref MAX_SOUNDS: u32 = {
        dotenv().unwrap();
        env::var("MAX_SOUNDS").unwrap().parse::<u32>().unwrap()
    };

    static ref PATREON_GUILD: u64 = {
        dotenv().unwrap();
        env::var("PATREON_GUILD").unwrap().parse::<u64>().unwrap()
    };

    static ref PATREON_ROLE: u64 = {
        dotenv().unwrap();
        env::var("PATREON_ROLE").unwrap().parse::<u64>().unwrap()
    };
}

#[group]
#[commands(info, help)]
struct AllUsers;

#[group]
#[commands(play, upload_new_sound, change_volume)]
#[checks(role_check)]
struct RoleManagedUsers;

#[group]
#[commands(change_prefix)]
#[checks(permission_check)]
struct PermissionManagedUsers;

#[check]
#[name("role_check")]
async fn role_check(ctx: &mut Context, msg: &Message, _args: &mut Args) -> CheckResult {

    async fn check_for_roles(ctx: &&mut Context, msg: &&Message) -> Result<(), Box<dyn std::error::Error>> {
        let pool = ctx.data.read().await
            .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

        let user_member = msg.member(&ctx).await;

        match user_member {
            Some(member) => {
                let user_roles: String = member.roles
                    .iter()
                    .map(|r| (*r.as_u64()).to_string())
                    .collect::<Vec<String>>()
                    .join(", ");

                let guild_id = *msg.guild_id.unwrap().as_u64();

                let res = sqlx::query!(
                "
SELECT COUNT(1) as count
    FROM roles
    WHERE guild_id = ? AND role IN (?)
                ",
                guild_id, user_roles
                ).fetch_one(&pool).await?;

                if res.count > 0 {
                    Ok(())
                }
                else {
                    Err(Box::new(ErrorTypes::NotEnoughRoles))
                }
            }

            None => {
                Err(Box::new(ErrorTypes::NotEnoughRoles))
            }
        }
    }

    if check_for_roles(&ctx, &msg).await.is_ok() {
        CheckResult::Success
    }
    else {
        perform_permission_check(ctx, &msg).await
    }
}

#[check]
#[name("permission_check")]
async fn permission_check(ctx: &mut Context, msg: &Message, _args: &mut Args) -> CheckResult {
    perform_permission_check(ctx, &msg).await
}

async fn perform_permission_check(ctx: &Context, msg: &&Message) -> CheckResult {
    if let Some(guild_id) = msg.guild_id {
        if let Ok(member) = guild_id.member(ctx.clone(), msg.author.id).await {
            if let Ok(perms) = member.permissions(ctx).await {
                if perms.manage_guild() {
                    return CheckResult::Success
                }
            }
        }
    }

    CheckResult::Failure(Reason::User(String::from("User needs `Manage Guild` permission")))
}

struct Sound {
    name: String,
    id: u32,
    plays: u32,
    public: bool,
    server_id: u64,
    uploader_id: u64,
    src: Vec<u8>,
}

#[derive(Debug)]
enum ErrorTypes {
    InvalidFile,
    NotEnoughRoles,
}

impl std::error::Error for ErrorTypes {}
impl std::fmt::Display for ErrorTypes {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ErrorTypes")
    }
}

impl Sound {
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
                Self,
                "
SELECT *
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
                Self,
                "
SELECT *
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

    async fn store_sound_source(&self) -> Result<String, Box<dyn std::error::Error>> {
        let caching_location = env::var("CACHING_LOCATION").unwrap_or(String::from("/tmp"));

        let path_name = format!("{}/sound-{}", caching_location, self.id);
        let path = Path::new(&path_name);

        if !path.exists() {
            use tokio::prelude::*;

            let mut file = File::create(&path).await?;

            file.write_all(self.src.as_ref()).await?;
        }

        Ok(path_name)
    }

    async fn commit(&self, db_pool: MySqlPool) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query!(
            "
UPDATE sounds
SET
    plays = ?,
    public = ?
WHERE
    id = ?
            ",
            self.plays, self.public, self.id
        )
            .execute(&db_pool)
            .await?;

        Ok(())
    }

    async fn count_user_sounds(user_id: u64, db_pool: MySqlPool) -> Result<u32, sqlx::error::Error> {
        let c = sqlx::query!(
        "
SELECT COUNT(1) as count
    FROM sounds
    WHERE uploader_id = ?
        ",
        user_id
        )
            .fetch_one(&db_pool)
            .await?.count;

        Ok(c as u32)
    }

    async fn create_anon(name: &str, src_url: &str, server_id: u64, user_id: u64, db_pool: MySqlPool) -> Result<u64, Box<dyn std::error::Error + Send>> {
        async fn process_src(src_url: &str) -> Option<Vec<u8>> {
            let future = Command::new("ffmpeg")
                .arg("-i")
                .arg(src_url)
                .arg("-loglevel")
                .arg("error")
                .arg("-b:a")
                .arg("28000")
                .arg("-f")
                .arg("opus")
                .arg("-fs")
                .arg("1048576")
                .arg("pipe:1")
                .output();

            let output = future.await;

            match output {
                Ok(out) => {
                    if out.status.success() {
                        Some(out.stdout)
                    }
                    else {
                        None
                    }
                }

                Err(_) => None,
            }
        }

        let source = process_src(src_url).await;

        match source {
            Some(data) => {
                match sqlx::query!(
                "
INSERT INTO sounds (name, server_id, uploader_id, public, src)
VALUES (?, ?, ?, 1, ?)
                ",
                name, server_id, user_id, data
                )
                    .execute(&db_pool)
                    .await {
                    Ok(u) => Ok(u),

                    Err(e) => Err(Box::new(e))
                }
            }

            None => Err(Box::new(ErrorTypes::InvalidFile))
        }
    }

    async fn get_user_sounds(user_id: u64, db_pool: MySqlPool) -> Result<Vec<Sound>, Box<dyn std::error::Error>> {
        let sounds = sqlx::query_as_unchecked!(
            Sound,
            "
SELECT *
    FROM sounds
    WHERE uploader_id = ?
            ",
            user_id
        ).fetch_all(&db_pool).await?;

        Ok(sounds)
    }

    async fn get_guild_sounds(guild_id: u64, db_pool: MySqlPool) -> Result<Vec<Sound>, Box<dyn std::error::Error>> {
        let sounds = sqlx::query_as_unchecked!(
            Sound,
            "
SELECT *
    FROM sounds
    WHERE server_id = ?
            ",
            guild_id
        ).fetch_all(&db_pool).await?;

        Ok(sounds)
    }
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
        .group(&ALLUSERS_GROUP)
        .group(&ROLEMANAGEDUSERS_GROUP)
        .group(&PERMISSIONMANAGEDUSERS_GROUP);

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

            let sound_res = Sound::search_for_sound(
                search_term,
                *guild_id.as_u64(),
                *msg.author.id.as_u64(),
                pool).await;

            match sound_res {
                Ok(sound) => {
                    let fp = sound.store_sound_source().await?;

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

#[command("prefix")]
async fn change_prefix(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let guild = match msg.guild(&ctx.cache).await {
        Some(guild) => guild,

        None => {
            return Ok(());
        }
    };

    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let mut guild_data;

    {
        let mut guild_data_opt = GuildData::get_from_id(*guild.read().await.id.as_u64(), pool.clone()).await;

        if guild_data_opt.is_none() {
            guild_data_opt = Some(GuildData::create_from_guild(guild.read().await, pool.clone()).await.unwrap())
        }

        guild_data = guild_data_opt.unwrap();
    }

    if args.len() == 1 {
        match args.single::<String>() {
            Ok(prefix) => {
                if prefix.len() <= 5 {
                    guild_data.prefix = prefix;

                    guild_data.commit(pool).await?;

                    msg.channel_id.say(&ctx, format!("Prefix changed to `{}`", guild_data.prefix)).await?;
                }
                else {
                    msg.channel_id.say(&ctx, "Prefix must be less than 5 characters long").await?;
                }
            }

            Err(_) => {
                msg.channel_id.say(&ctx, format!("Usage: `{prefix}prefix <new prefix>`", prefix = guild_data.prefix)).await?;
            }
        }
    }
    else {
        msg.channel_id.say(&ctx, format!("Usage: `{prefix}prefix <new prefix>`", prefix = guild_data.prefix)).await?;
    }

    Ok(())
}

#[command("upload")]
async fn upload_new_sound(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let new_name = args.rest();

    if !new_name.is_empty() && new_name.len() <= 20 {
        let pool = ctx.data.read().await
            .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

        // need to check how many sounds user currently has
        let count = Sound::count_user_sounds(*msg.author.id.as_u64(), pool.clone()).await?;
        let mut permit_upload = true;

        // need to check if user is patreon or nah
        if count >= *MAX_SOUNDS {
            let patreon_guild_member = GuildId(*PATREON_GUILD).member(&ctx, msg.author.id).await?;

            if patreon_guild_member.roles.contains(&RoleId(*PATREON_ROLE)) {
                permit_upload = true;
            }
            else {
                permit_upload = false;
            }
        }

        if permit_upload {
            msg.channel_id.say(&ctx, "Please now upload an audio file under 1MB in size (larger files will be automatically trimmed):").await?;

            let reply = msg.channel_id.await_reply(&ctx)
                .author_id(msg.author.id)
                .timeout(Duration::from_secs(30))
                .await;

            match reply {
                Some(reply_msg) => {
                    if reply_msg.attachments.len() == 1 {
                        match Sound::create_anon(
                            new_name,
                            &reply_msg.attachments[0].url,
                            *msg.guild_id.unwrap().as_u64(),
                            *msg.author.id.as_u64(),
                            pool).await {
                            Ok(_) => {
                                msg.channel_id.say(&ctx, "Sound has been uploaded").await?;
                            }

                            Err(_) => {
                                msg.channel_id.say(&ctx, "Sound failed to upload.").await?;
                            }
                        }
                    } else {
                        msg.channel_id.say(&ctx, "Please upload 1 attachment following your upload command. Aborted").await?;
                    }
                }

                None => {
                    msg.channel_id.say(&ctx, "Upload timed out. Please redo the command").await?;
                }
            }
        }
        else {
            msg.channel_id.say(
                &ctx,
                format!(
                    "You have reached the maximum number of sounds ({}). Either delete some with `?delete` or join our Patreon for unlimited uploads at **https://patreon.com/jellywx**",
                    *MAX_SOUNDS,
                )).await?;
        }
    }
    else {
        msg.channel_id.say(&ctx, "Usage: `?upload <name>`. Please ensure the name provided is less than 20 characters in length").await?;
    }

    Ok(())
}

#[command]
async fn set_allowed_roles(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    if args.len() == 0 {
        msg.channel_id.say(&ctx, "Usage: `?roles <role mentions or anything else to disable>`. Current roles: ").await?;
    }
    else {
        let pool = ctx.data.read().await
            .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

        let guild_id = *msg.guild_id.unwrap().as_u64();

        sqlx::query!(
            "
DELETE FROM roles
    WHERE guild_id = ?
            ",
            guild_id
            ).execute(&pool).await?;

        if msg.mention_roles.len() > 0 {
            for role in msg.mention_roles.iter().map(|r| *r.as_u64()) {
                sqlx::query!(
                "
INSERT INTO roles (guild_id, role)
    VALUES
        (?, ?)
                ",
                guild_id, role
                ).execute(&pool).await?;
            }

            msg.channel_id.say(&ctx, "Specified roles whitelisted").await?;
        }
        else {
            msg.channel_id.say(&ctx, "Role whitelisting disabled").await?;
        }
    }

    Ok(())
}

#[command]
async fn list_sounds(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let pool = ctx.data.read().await
            .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let sounds;

    if args.rest() == "me" {
        sounds = Sound::get_user_sounds(*msg.author.id.as_u64(), pool).await?;
    }
    else {
        sounds = Sound::get_guild_sounds(*msg.guild_id.unwrap().as_u64(), pool).await?;
    }

    Ok(())
}
