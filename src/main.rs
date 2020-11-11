#[macro_use]
extern crate lazy_static;

extern crate reqwest;

mod error;
mod guilddata;
mod sound;

use guilddata::GuildData;
use sound::Sound;

use serenity::{
    client::{
        bridge::{gateway::GatewayIntents, voice::ClientVoiceManager},
        Client, Context,
    },
    framework::standard::{
        macros::{check, command, group, hook},
        Args, CheckResult, CommandError, CommandResult, DispatchError, Reason, StandardFramework,
    },
    http::Http,
    model::{
        channel::{Channel, Message},
        guild::Guild,
        id::{GuildId, RoleId},
        voice::VoiceState,
    },
    prelude::{Mutex as SerenityMutex, *},
    utils::shard_id,
    voice::Handler as VoiceHandler,
};

use sqlx::mysql::MySqlPool;

use dotenv::dotenv;

use std::{collections::HashMap, env, sync::Arc, time::Duration};

struct MySQL;

impl TypeMapKey for MySQL {
    type Value = MySqlPool;
}

struct VoiceManager;

impl TypeMapKey for VoiceManager {
    type Value = Arc<SerenityMutex<ClientVoiceManager>>;
}

struct ReqwestClient;

impl TypeMapKey for ReqwestClient {
    type Value = Arc<reqwest::Client>;
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
#[commands(
    info,
    help,
    list_sounds,
    change_public,
    search_sounds,
    show_popular_sounds,
    show_random_sounds,
    set_greet_sound
)]
#[checks(self_perm_check)]
struct AllUsers;

#[group]
#[commands(play, upload_new_sound, change_volume, delete_sound, stop_playing)]
#[checks(self_perm_check, role_check)]
struct RoleManagedUsers;

#[group]
#[commands(change_prefix, set_allowed_roles, allow_greet_sounds)]
#[checks(self_perm_check, permission_check)]
struct PermissionManagedUsers;

#[check]
#[name("self_perm_check")]
async fn self_perm_check(ctx: &Context, msg: &Message, _args: &mut Args) -> CheckResult {
    let channel_o = msg.channel(&ctx).await;

    if let Some(channel_e) = channel_o {
        if let Channel::Guild(channel) = channel_e {
            let permissions_r = channel
                .permissions_for_user(&ctx, &ctx.cache.current_user_id().await)
                .await;

            if let Ok(permissions) = permissions_r {
                if permissions.send_messages() && permissions.embed_links() {
                    CheckResult::Success
                } else {
                    CheckResult::Failure(Reason::Log(
                        "Bot does not have enough permissions".to_string(),
                    ))
                }
            } else {
                CheckResult::Failure(Reason::Log("No perms found".to_string()))
            }
        } else {
            CheckResult::Failure(Reason::Log("No DM commands".to_string()))
        }
    } else {
        CheckResult::Failure(Reason::Log("Channel not available".to_string()))
    }
}

#[check]
#[name("role_check")]
async fn role_check(ctx: &Context, msg: &Message, _args: &mut Args) -> CheckResult {
    async fn check_for_roles(ctx: &&Context, msg: &&Message) -> CheckResult {
        let pool = ctx
            .data
            .read()
            .await
            .get::<MySQL>()
            .cloned()
            .expect("Could not get SQLPool from data");

        let guild_opt = msg.guild(&ctx).await;

        match guild_opt {
            Some(guild) => {
                let member_res = guild.member(*ctx, msg.author.id).await;

                match member_res {
                    Ok(member) => {
                        let user_roles: String = member
                            .roles
                            .iter()
                            .map(|r| (*r.as_u64()).to_string())
                            .collect::<Vec<String>>()
                            .join(", ");

                        let guild_id = *msg.guild_id.unwrap().as_u64();

                        let role_res = sqlx::query!(
                            "
SELECT COUNT(1) as count
    FROM roles
    WHERE
        (guild_id = ? AND role IN (?)) OR
        (role = ?)
                            ",
                            guild_id,
                            user_roles,
                            guild_id
                        )
                        .fetch_one(&pool)
                        .await;

                        match role_res {
                            Ok(role_count) => {
                                if role_count.count > 0 {
                                    CheckResult::Success
                                }
                                else {
                                    CheckResult::Failure(Reason::User("User has not got a sufficient role. Use `?roles` to set up role restrictions".to_string()))
                                }
                            }

                            Err(_) => {
                                CheckResult::Failure(Reason::User("User has not got a sufficient role. Use `?roles` to set up role restrictions".to_string()))
                            }
                        }
                    }

                    Err(_) => CheckResult::Failure(Reason::User(
                        "Unexpected error looking up user roles".to_string(),
                    )),
                }
            }

            None => CheckResult::Failure(Reason::User(
                "Unexpected error looking up guild".to_string(),
            )),
        }
    }

    if perform_permission_check(ctx, &msg).await.is_success() {
        CheckResult::Success
    } else {
        check_for_roles(&ctx, &msg).await
    }
}

#[check]
#[name("permission_check")]
async fn permission_check(ctx: &Context, msg: &Message, _args: &mut Args) -> CheckResult {
    perform_permission_check(ctx, &msg).await
}

async fn perform_permission_check(ctx: &Context, msg: &&Message) -> CheckResult {
    if let Some(guild) = msg.guild(&ctx).await {
        if guild
            .member_permissions(&ctx, &msg.author)
            .await
            .unwrap()
            .manage_guild()
        {
            CheckResult::Success
        } else {
            CheckResult::Failure(Reason::User(String::from(
                "User needs `Manage Guild` permission",
            )))
        }
    } else {
        CheckResult::Failure(Reason::User(String::from("Guild not cached")))
    }
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
        if let (Some(guild_id), Some(user_channel)) = (guild_id_opt, new.channel_id) {
            if old.is_none() {
                if let Some(guild) = ctx.cache.guild(guild_id).await {
                    let pool = ctx
                        .data
                        .read()
                        .await
                        .get::<MySQL>()
                        .cloned()
                        .expect("Could not get SQLPool from data");

                    let guild_data_opt = GuildData::get_from_id(guild, pool.clone()).await;

                    if let Some(guild_data) = guild_data_opt {
                        if guild_data.allow_greets {
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

                                let voice_manager_lock = ctx
                                    .data
                                    .read()
                                    .await
                                    .get::<VoiceManager>()
                                    .cloned()
                                    .expect("Could not get VoiceManager from data");

                                let mut voice_manager = voice_manager_lock.lock().await;

                                if let Some(handler) = voice_manager.join(guild_id, user_channel) {
                                    let _audio =
                                        play_audio(&mut sound, guild_data, handler, pool).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn play_audio(
    sound: &mut Sound,
    guild: GuildData,
    handler: &mut VoiceHandler,
    mysql_pool: MySqlPool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let audio = handler.play_only(sound.store_sound_source(mysql_pool.clone()).await?);

    {
        let mut locked = audio.lock().await;

        locked.volume(guild.volume as f32 / 100.0);
    }

    sound.plays += 1;
    sound.commit(mysql_pool).await?;

    Ok(())
}

#[hook]
async fn log_errors(_: &Context, m: &Message, cmd_name: &str, error: Result<(), CommandError>) {
    if let Err(e) = error {
        println!("Error in command {} ({}): {:?}", cmd_name, m.content, e);
    }
}

#[hook]
async fn dispatch_error_hook(ctx: &Context, msg: &Message, error: DispatchError) {
    match error {
        DispatchError::CheckFailed(_f, reason) => {
            if let Reason::User(description) = reason {
                let _ = msg
                    .reply(ctx, format!("You cannot do this command: {}", description))
                    .await;
            }
        }

        _ => {}
    }
}

// entry point
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenv()?;

    let token = env::var("DISCORD_TOKEN").expect("Missing DISCORD_TOKEN from environment");

    let http = Http::new_with_token(&token);

    let logged_in_id = http.get_current_user().await?.id;

    let framework = StandardFramework::new()
        .configure(|c| {
            c.dynamic_prefix(|ctx, msg| {
                Box::pin(async move {
                    let pool = ctx
                        .data
                        .read()
                        .await
                        .get::<MySQL>()
                        .cloned()
                        .expect("Could not get SQLPool from data");

                    let guild = match msg.guild(&ctx.cache).await {
                        Some(guild) => guild,

                        None => {
                            return Some(String::from("?"));
                        }
                    };

                    match GuildData::get_from_id(guild.clone(), pool.clone()).await {
                        Some(mut guild_data) => {
                            let name = Some(guild.name);

                            if guild_data.name != name {
                                guild_data.name = name;
                                guild_data.commit(pool).await.unwrap();
                            }
                            Some(guild_data.prefix)
                        }

                        None => {
                            GuildData::create_from_guild(guild, pool).await.unwrap();
                            Some(String::from("?"))
                        }
                    }
                })
            })
            .allow_dm(false)
            .ignore_bots(true)
            .ignore_webhooks(true)
            .on_mention(Some(logged_in_id))
        })
        .group(&ALLUSERS_GROUP)
        .group(&ROLEMANAGEDUSERS_GROUP)
        .group(&PERMISSIONMANAGEDUSERS_GROUP)
        .after(log_errors)
        .on_dispatch_error(dispatch_error_hook);

    let mut client =
        Client::builder(&env::var("DISCORD_TOKEN").expect("Missing token from environment"))
            .intents(
                GatewayIntents::GUILD_VOICE_STATES
                    | GatewayIntents::GUILD_MESSAGES
                    | GatewayIntents::GUILDS,
            )
            .framework(framework)
            .event_handler(Handler)
            .await
            .expect("Error occurred creating client");

    {
        let mysql_pool =
            MySqlPool::new(&env::var("DATABASE_URL").expect("No database URL provided"))
                .await
                .unwrap();

        let mut data = client.data.write().await;

        data.insert::<MySQL>(mysql_pool);

        data.insert::<VoiceManager>(Arc::clone(&client.voice_manager));

        data.insert::<ReqwestClient>(Arc::new(reqwest::Client::new()));
    }

    client.start_autosharded().await?;

    Ok(())
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
                    let voice_manager_lock = ctx
                        .data
                        .read()
                        .await
                        .get::<VoiceManager>()
                        .cloned()
                        .expect("Could not get VoiceManager from data");

                    let mut voice_manager = voice_manager_lock.lock().await;

                    match voice_manager.join(guild_id, user_channel) {
                        Some(handler) => {
                            let guild_data =
                                GuildData::get_from_id(guild, pool.clone()).await.unwrap();

                            play_audio(sound, guild_data, handler, pool).await?;

                            msg.channel_id
                                .say(
                                    &ctx,
                                    format!("Playing sound {} with ID {}", sound.name, sound.id),
                                )
                                .await?;
                        }

                        None => {
                            msg.channel_id.say(&ctx, "Failed to join channel").await?;
                        }
                    };
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

#[command]
async fn help(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    msg.channel_id
        .send_message(&ctx, |m| {
            m.embed(|e| {
                e.title("Help")
                    .color(THEME_COLOR)
                    .description("Please visit our website at https://soundfx.jellywx.com/help")
            })
        })
        .await?;

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

Reset prefix: `<@{0}> prefix ?`

Invite me: https://discordapp.com/oauth2/authorize?client_id={1}&scope=bot&permissions=36703232

**Welcome to SoundFX!**
Developer: <@203532103185465344>
Find me on https://discord.jellywx.com/ and on https://github.com/JellyWX :)

**An online dashboard is available!** Visit https://soundfx.jellywx.com/dashboard
There is a maximum sound limit per user. This can be removed by donating at https://patreon.com/jellywx", current_user.name, current_user.id.as_u64())))).await?;

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

    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let guild_data_opt = GuildData::get_from_id(guild, pool.clone()).await;
    let mut guild_data = guild_data_opt.unwrap();

    if args.len() == 1 {
        match args.single::<u8>() {
            Ok(volume) => {
                guild_data.volume = volume;

                guild_data.commit(pool).await?;

                msg.channel_id
                    .say(&ctx, format!("Volume changed to {}%", volume))
                    .await?;
            }

            Err(_) => {
                msg.channel_id.say(&ctx,
                                   format!("Current server volume: {vol}%. Change the volume with ```{prefix}volume <new volume>```",
                                           vol = guild_data.volume, prefix = guild_data.prefix)).await?;
            }
        }
    } else {
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

    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let mut guild_data;

    {
        let guild_data_opt = GuildData::get_from_id(guild, pool.clone()).await;

        guild_data = guild_data_opt.unwrap();
    }

    if args.len() == 1 {
        match args.single::<String>() {
            Ok(prefix) => {
                if prefix.len() <= 5 {
                    guild_data.prefix = prefix;

                    guild_data.commit(pool).await?;

                    msg.channel_id
                        .say(&ctx, format!("Prefix changed to `{}`", guild_data.prefix))
                        .await?;
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
                            prefix = guild_data.prefix
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
                    prefix = guild_data.prefix
                ),
            )
            .await?;
    }

    Ok(())
}

#[command("upload")]
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

#[command("roles")]
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

#[command("list")]
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

#[command("public")]
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

#[command("delete")]
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

#[command("search")]
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

#[command("popular")]
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

#[command("random")]
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

#[command("greet")]
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

#[command("stop")]
async fn stop_playing(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let voice_manager_lock = ctx
        .data
        .read()
        .await
        .get::<VoiceManager>()
        .cloned()
        .expect("Could not get VoiceManager from data");

    let mut voice_manager = voice_manager_lock.lock().await;

    let manager_opt = voice_manager.get_mut(msg.guild_id.unwrap());

    if let Some(manager) = manager_opt {
        manager.leave();
    }

    Ok(())
}

#[command("allow_greet")]
async fn allow_greet_sounds(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild = match msg.guild(&ctx.cache).await {
        Some(guild) => guild,

        None => {
            return Ok(());
        }
    };

    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not acquire SQL pool from data");

    let guild_data_opt = GuildData::get_from_id(guild, pool.clone()).await;

    if let Some(mut guild_data) = guild_data_opt {
        guild_data.allow_greets = !guild_data.allow_greets;

        guild_data.commit(pool).await?;

        msg.channel_id
            .say(
                &ctx,
                format!(
                    "Greet sounds have been {}abled in this server",
                    if guild_data.allow_greets { "en" } else { "dis" }
                ),
            )
            .await?;
    }

    Ok(())
}
