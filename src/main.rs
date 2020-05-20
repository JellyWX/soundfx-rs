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
        channel::Message,
        guild::Guild,
        id::{
            GuildId,
            RoleId,
        },
        voice::VoiceState,
    },
    prelude::{
        Mutex as SerenityMutex,
        *
    },
    voice::{
        AudioSource,
        ffmpeg,
        Handler as VoiceHandler,
    },
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
    sync::{
        Mutex,
        MutexGuard
    },
    time,
};

use std::{
    collections::{
        HashMap,
        HashSet,
    },
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

struct VoiceGuilds;

impl TypeMapKey for VoiceGuilds {
    type Value = Arc<Mutex<HashMap<GuildId, u8>>>;
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

    static ref DISCONNECT_CYCLES: u8 = {
        dotenv().unwrap();
        env::var("DISCONNECT_CYCLES").unwrap_or("2").parse::<u64>().unwrap()
    };
}

#[group]
#[commands(info, help, list_sounds, change_public, search_sounds, show_popular_sounds, show_random_sounds, set_greet_sound)]
struct AllUsers;

#[group]
#[commands      (play, upload_new_sound, change_volume, delete_sound)]
#[checks(role_check)]
struct RoleManagedUsers;

#[group]
#[commands(change_prefix, set_allowed_roles)]
#[checks(permission_check)]
struct PermissionManagedUsers;

#[check]
#[name("role_check")]
async fn role_check(ctx: &Context, msg: &Message, _args: &mut Args) -> CheckResult {

    async fn check_for_roles(ctx: &&Context, msg: &&Message) -> Result<(), Box<dyn std::error::Error>> {
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
async fn permission_check(ctx: &Context, msg: &Message, _args: &mut Args) -> CheckResult {
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
    async fn search_for_sound(query: &str, guild_id: u64, user_id: u64, db_pool: MySqlPool, strict: bool) -> Result<Vec<Sound>, sqlx::Error> {

        fn extract_id(s: &str) -> Option<u32> {
            if s.len() > 3 && s.to_lowercase().starts_with("id:") {
                match s[3..].parse::<u32>() {
                    Ok(id) => Some(id),

                    Err(_) => None
                }
            }
            else {
                None
            }
        }

        if let Some(id) = extract_id(&query) {
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
                ",
                id, user_id, guild_id
            )
                .fetch_all(&db_pool)
                .await?;

            Ok(sound)
        }
        else {
            let name = query;
            let sound;

            if strict {
                sound = sqlx::query_as_unchecked!(
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
                    ",
                    name, user_id, guild_id, guild_id, user_id
                )
                    .fetch_all(&db_pool)
                    .await?;

            }
            else {
                sound = sqlx::query_as_unchecked!(
                    Self,
                    "
SELECT *
    FROM sounds
    WHERE name LIKE CONCAT('%', ?, '%') AND (
        public = 1 OR
        uploader_id = ? OR
        server_id = ?
    )
    ORDER BY rand(), public = 1, server_id = ?, uploader_id = ?
                    ",
                    name, user_id, guild_id, guild_id, user_id
                )
                    .fetch_all(&db_pool)
                    .await?;
            }

            Ok(sound)
        }
    }

    async fn store_sound_source(&self) -> Result<Box<dyn AudioSource>, Box<dyn std::error::Error>> {
        let caching_location = env::var("CACHING_LOCATION").unwrap_or(String::from("/tmp"));

        let path_name = format!("{}/sound-{}", caching_location, self.id);
        let path = Path::new(&path_name);

        if !path.exists() {
            use tokio::prelude::*;

            let mut file = File::create(&path).await?;

            file.write_all(self.src.as_ref()).await?;
        }

        Ok(ffmpeg(path_name).await?)
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

    async fn count_named_user_sounds(user_id: u64, name: &String, db_pool: MySqlPool) -> Result<u32, sqlx::error::Error> {
        let c = sqlx::query!(
        "
SELECT COUNT(1) as count
    FROM sounds
    WHERE
        uploader_id = ? AND
        name = ?
        ",
        user_id, name
        )
            .fetch_one(&db_pool)
            .await?.count;

        Ok(c as u32)
    }

    async fn set_as_greet(&self, user_id: u64, db_pool: MySqlPool) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query!(
            "
UPDATE users
SET
    join_sound_id = ?
WHERE
    user = ?
            ",
            self.id, user_id
        )
            .execute(&db_pool)
            .await?;

        Ok(())
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

    async fn delete(&self, db_pool: MySqlPool) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query!(
            "
DELETE
    FROM sounds
    WHERE id = ?
            ",
            self.id
        )
            .execute(&db_pool)
            .await?;

        Ok(())
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
    pub id: u64,
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

    async fn create_from_guild(guild: Guild, db_pool: MySqlPool) -> Result<GuildData, Box<dyn std::error::Error>> {
        sqlx::query!(
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
impl EventHandler for Handler {
    async fn voice_state_update(&self, ctx: Context, guild_id_opt: Option<GuildId>, old: Option<VoiceState>, new: VoiceState) {
        if let (Some(guild_id), Some(user_channel)) = (guild_id_opt, new.channel_id) {
            let pool = ctx.data.read().await
                .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

            if old.is_none() {
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

                match join_id_res {
                    Ok(join_id_record) => {
                        let join_id = join_id_record.join_sound_id;
                        let mut sound = sqlx::query_as_unchecked!(
                            Sound,
                            "
SELECT *
    FROM sounds
    WHERE id = ?
                            ",
                            join_id
                        )
                            .fetch_one(&pool)
                            .await.unwrap();

                        let voice_manager_lock = ctx.data.read().await
                            .get::<VoiceManager>().cloned().expect("Could not get VoiceManager from data");

                        let mut voice_manager = voice_manager_lock.lock().await;

                        let voice_guilds_lock = ctx.data.read().await
                            .get::<VoiceGuilds>().cloned().expect("Could not get VoiceGuilds from data");

                        let voice_guilds = voice_guilds_lock.lock().await;

                        let guild_data = GuildData::get_from_id(*guild_id.as_u64(), pool.clone()).await.unwrap();

                        if let Some(handler) = voice_manager.join(guild_id, user_channel) {
                            let _audio = play_audio(&mut sound, guild_data, handler, voice_guilds, pool).await;
                        }
                    }

                    Err(_) => {}
                }
            }
        }
    }
}

async fn play_audio(sound: &mut Sound, guild: GuildData, handler: &mut VoiceHandler, mut voice_guilds: MutexGuard<'_, HashMap<GuildId, u8>>, pool: MySqlPool)
    -> Result<(), Box<dyn std::error::Error>> {

    let audio = handler.play_only(sound.store_sound_source().await?);

    {
        let mut locked = audio.lock().await;

        locked.volume(guild.volume as f32 / 100.0);
    }

    sound.plays += 1;
    sound.commit(pool).await?;

    voice_guilds.insert(GuildId(guild.id), *DISCONNECT_CYCLES);

    Ok(())
}

// entry point
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv()?;

    let voice_guilds = Arc::new(Mutex::new(HashMap::new()));

    let framework = StandardFramework::new()
        .configure(|c| c
            .dynamic_prefix(|ctx, msg| Box::pin(async move {
                let pool = ctx.data.read().await
                    .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

                match GuildData::get_from_id(*msg.guild_id.unwrap().as_u64(), pool).await {
                    Some(guild) => Some(guild.prefix),

                    None => Some(String::from("?"))
                }
            }))
            .allow_dm(false)
            .ignore_bots(true)
            .ignore_webhooks(true)
        )
        .group(&ALLUSERS_GROUP)
        .group(&ROLEMANAGEDUSERS_GROUP)
        .group(&PERMISSIONMANAGEDUSERS_GROUP);

    let mut client = Client::new(&env::var("DISCORD_TOKEN").expect("Missing token from environment"))
        .intents(GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILDS)
        .framework(framework)
        .event_handler(Handler)
        .await.expect("Error occurred creating client");

    {
        let pool = MySqlPool::new(&env::var("DATABASE_URL").expect("No database URL provided")).await.unwrap();

        let mut data = client.data.write().await;
        data.insert::<SQLPool>(pool);

        data.insert::<VoiceManager>(Arc::clone(&client.voice_manager));

        data.insert::<VoiceGuilds>(voice_guilds.clone());
    }

    let cvm = Arc::clone(&client.voice_manager);

    let disconnect_cycle_delay = env::var("DISCONNECT_CYCLE_DELAY")
        .unwrap_or("300".to_string())
        .parse::<u64>()?;

    // select on the client and client auto disconnector (when the client terminates, terminate the disconnector
    tokio::select! {
        _ = client.start() => {}
        _ = disconnect_from_inactive(cvm, voice_guilds, disconnect_cycle_delay) => {}
    };

    Ok(())
}

async fn disconnect_from_inactive(voice_manager_mutex: Arc<SerenityMutex<ClientVoiceManager>>, voice_guilds: Arc<Mutex<HashMap<GuildId, u8>>>, wait_time: u64) {
    loop {
        time::delay_for(Duration::from_secs(wait_time)).await;

        let mut voice_guilds_acquired = voice_guilds.lock().await;
        let mut voice_manager = voice_manager_mutex.lock().await;

        let mut to_remove = HashSet::new();

        for (guild, ticker) in voice_guilds_acquired.iter_mut() {
            if *ticker == 0 {
                let manager_opt = voice_manager.get_mut(guild);

                if let Some(manager) = manager_opt {
                    manager.leave();
                    to_remove.insert(guild.clone());
                }
                else {
                    to_remove.insert(guild.clone());
                }
            }
            else {
                *ticker -= 1;
            }
        }

        for val in to_remove.iter() {
            voice_guilds_acquired.remove(val);
        }
    }
}

#[command("play")]
#[aliases("p")]
async fn play(ctx: &Context, msg: &Message, args: Args) -> CommandResult {

    let guild = match msg.guild(&ctx.cache).await {
        Some(guild) => guild,

        None => {
            return Ok(());
        }
    };

    let guild_id = guild.id;

    let channel_to_join = guild
        .voice_states.get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    match channel_to_join {
        Some(user_channel) => {
            let search_term = args.rest();

            let pool = ctx.data.read().await
                .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

            let mut sound_vec = Sound::search_for_sound(
                search_term,
                *guild_id.as_u64(),
                *msg.author.id.as_u64(),
                pool.clone(),
                true).await?;

            let sound_res = sound_vec.first_mut();

            match sound_res {
                Some(sound) => {
                    let voice_manager_lock = ctx.data.read().await
                        .get::<VoiceManager>().cloned().expect("Could not get VoiceManager from data");

                    let mut voice_manager = voice_manager_lock.lock().await;

                    let voice_guilds_lock = ctx.data.read().await
                        .get::<VoiceGuilds>().cloned().expect("Could not get VoiceGuilds from data");

                    let voice_guilds = voice_guilds_lock.lock().await;

                    let guild_data = GuildData::get_from_id(*guild_id.as_u64(), pool.clone()).await.unwrap();

                    match voice_manager.join(guild_id, user_channel) {
                        Some(handler) => {
                            play_audio(sound, guild_data, handler, voice_guilds, pool).await?;
                        }

                        None => {
                            msg.channel_id.say(&ctx, "Failed to join channel").await?;
                        }
                    };
                }

                None => {
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
async fn help(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    msg.channel_id.send_message(&ctx, |m| m
        .embed(|e| e
            .title("Help")
            .color(THEME_COLOR)
            .description("Please visit our website at https://soundfx.jellywx.com/help"))).await?;

    Ok(())
}

#[command]
async fn info(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {

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
async fn change_volume(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let guild = match msg.guild(&ctx.cache).await {
        Some(guild) => guild,

        None => {
            return Ok(());
        }
    };

    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let mut guild_data_opt = GuildData::get_from_id(*guild.id.as_u64(), pool.clone()).await;

    if guild_data_opt.is_none() {
        guild_data_opt = Some(GuildData::create_from_guild(guild, pool.clone()).await.unwrap())
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
async fn change_prefix(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
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
        let mut guild_data_opt = GuildData::get_from_id(*guild.id.as_u64(), pool.clone()).await;

        if guild_data_opt.is_none() {
            guild_data_opt = Some(GuildData::create_from_guild(guild, pool.clone()).await.unwrap())
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
async fn upload_new_sound(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let new_name = args.rest().to_string();

    if !new_name.is_empty() && new_name.len() <= 20 {
        let pool = ctx.data.read().await
            .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

        // need to check the name is not currently in use by the user
        let count_name = Sound::count_named_user_sounds(*msg.author.id.as_u64(), &new_name, pool.clone()).await?;
        if count_name > 0 {
            msg.channel_id.say(&ctx, "You are already using that name. Please choose a unique name for your upload.").await?;
        }

        else {
            // need to check how many sounds user currently has
            let count = Sound::count_user_sounds(*msg.author.id.as_u64(), pool.clone()).await?;
            let mut permit_upload = true;

            // need to check if user is patreon or nah
            if count >= *MAX_SOUNDS {
                let patreon_guild_member = GuildId(*PATREON_GUILD).member(ctx, msg.author.id).await?;

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
                                &new_name,
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
    }
    else {
        msg.channel_id.say(&ctx, "Usage: `?upload <name>`. Please ensure the name provided is less than 20 characters in length").await?;
    }

    Ok(())
}

#[command("roles")]
async fn set_allowed_roles(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
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

#[command("list")]
async fn list_sounds(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let sounds;
    let mut message_buffer;

    if args.rest() == "me" {
        sounds = Sound::get_user_sounds(*msg.author.id.as_u64(), pool).await?;

        message_buffer = "All your sounds: ".to_string();
    }
    else {
        sounds = Sound::get_guild_sounds(*msg.guild_id.unwrap().as_u64(), pool).await?;

        message_buffer = "All sounds on this server: ".to_string();
    }

    for sound in sounds {
        message_buffer.push_str(format!("**{}** ({}), ", sound.name, if sound.public { "🔓" } else { "🔒" }).as_str());

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

#[command("public")]
async fn change_public(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");
    let uid = msg.author.id.as_u64();

    let name = args.rest();
    let gid = *msg.guild_id.unwrap().as_u64();

    let mut sound_vec = Sound::search_for_sound(name, gid, *uid, pool.clone(), true).await?;
    let sound_result = sound_vec.first_mut();

    match sound_result {
        Some(sound) => {
            if sound.uploader_id != *uid {
                msg.channel_id.say(&ctx, "You can only change the availability of sounds you have uploaded. Use `?list me` to view your sounds").await?;
            }

            else {
                if sound.public {
                    sound.public = false;

                    msg.channel_id.say(&ctx, "Sound has been set to private 🔒").await?;
                } else {
                    sound.public = true;

                    msg.channel_id.say(&ctx, "Sound has been set to public 🔓").await?;
                }

                sound.commit(pool).await?
            }
        }

        None => {
            msg.channel_id.say(&ctx, "Sound could not be found by that name.").await?;
        }
    }

    Ok(())
}

#[command("delete")]
async fn delete_sound(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let uid = *msg.author.id.as_u64();
    let gid = *msg.guild_id.unwrap().as_u64();

    let name = args.rest();

    let sound_vec = Sound::search_for_sound(name, gid, uid, pool.clone(), true).await?;
    let sound_result = sound_vec.first();

    match sound_result {
        Some(sound) => {
            if sound.uploader_id != uid && sound.server_id != gid {
                msg.channel_id.say(&ctx, "You can only delete sounds from this guild or that you have uploaded.").await?;
            }

            else {
                sound.delete(pool).await?;

                msg.channel_id.say(&ctx, "Sound has been deleted").await?;
            }
        }

        None => {
            msg.channel_id.say(&ctx, "Sound could not be found by that name.").await?;
        }
    }

    Ok(())
}

async fn format_search_results(search_results: Vec<Sound>, msg: &Message, ctx: &Context) -> Result<(), Box<dyn std::error::Error>> {
    let mut current_character_count = 0;
    let title = "Public sounds matching filter:";

    let field_iter = search_results.iter().take(25).map(|item| {

        (&item.name, format!("ID: {}\nPlays: {}", item.id, item.plays), false)

    }).filter(|item| {

        current_character_count += item.0.len() + item.1.len();

        current_character_count <= 2048 - title.len()

    });

    msg.channel_id.send_message(&ctx, |m| {
        m.embed(|e| { e
                .title(title)
                .fields(field_iter)
        })
    }).await?;

    Ok(())
}

#[command("search")]
async fn search_sounds(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let query = args.rest();

    let search_results = Sound::search_for_sound(query, *msg.guild_id.unwrap().as_u64(), *msg.author.id.as_u64(), pool, false).await?;

    format_search_results(search_results, msg, ctx).await?;

    Ok(())
}

#[command("popular")]
async fn show_popular_sounds(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let search_results = sqlx::query_as_unchecked!(
        Sound,
        "
SELECT * FROM sounds
    ORDER BY plays DESC
    LIMIT 25
        "
    )
        .fetch_all(&pool)
        .await?;

    format_search_results(search_results, msg, ctx).await?;

    Ok(())
}

#[command("random")]
async fn show_random_sounds(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let search_results = sqlx::query_as_unchecked!(
        Sound,
        "
SELECT * FROM sounds
    ORDER BY rand()
    LIMIT 25
        "
    )
        .fetch_all(&pool)
        .await?;

    format_search_results(search_results, msg, ctx).await?;

    Ok(())
}

#[command("greet")]
async fn set_greet_sound(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let query = args.rest();
    let user_id = *msg.author.id.as_u64();

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

        msg.channel_id.say(&ctx, "Your greet sound has been unset.").await?;
    }
    else {

        let sound_vec = Sound::search_for_sound(query, *msg.guild_id.unwrap().as_u64(), user_id, pool.clone(), true).await?;

        match sound_vec.first() {
            Some(sound) => {
                sound.set_as_greet(user_id, pool).await?;

                msg.channel_id.say(&ctx, format!("Greet sound has been set to {} (ID {})", sound.name, sound.id)).await?;
            }

            None => {
                msg.channel_id.say(&ctx, "Could not find a sound by that name.").await?;
            }
        }
    }

    Ok(())
}
