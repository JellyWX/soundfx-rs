use regex_command_attr::command;

use serenity::{client::Context, framework::standard::CommandResult};

use crate::{
    framework::{Args, CommandInvoke, CreateGenericResponse},
    guild_data::CtxGuildData,
    sound::{JoinSoundCtx, Sound},
    MySQL,
};

#[command("volume")]
#[aliases("vol")]
#[required_permissions(Managed)]
#[group("Settings")]
#[description("Change the bot's volume in this server")]
#[arg(
    name = "volume",
    description = "New volume for the bot to use",
    kind = "Integer",
    required = false
)]
pub async fn change_volume(
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

    let guild_data_opt = ctx.guild_data(invoke.guild_id().unwrap()).await;
    let guild_data = guild_data_opt.unwrap();

    if let Some(volume) = args.named("volume").map(|i| i.parse::<u8>().ok()).flatten() {
        guild_data.write().await.volume = volume;

        guild_data.read().await.commit(pool).await?;

        invoke
            .respond(
                ctx.http.clone(),
                CreateGenericResponse::new().content(format!("Volume changed to {}%", volume)),
            )
            .await?;
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
#[allow_slash(false)]
#[group("Settings")]
#[description("Change the prefix of the bot for using non-slash commands")]
#[arg(
    name = "prefix",
    kind = "String",
    description = "The new prefix to use for the bot",
    required = true
)]
pub async fn change_prefix(
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

    let guild_data;

    {
        let guild_data_opt = ctx.guild_data(invoke.guild_id().unwrap()).await;

        guild_data = guild_data_opt.unwrap();
    }

    if let Some(prefix) = args.named("prefix") {
        if prefix.len() <= 5 {
            let reply = format!("Prefix changed to `{}`", prefix);

            {
                guild_data.write().await.prefix = prefix.to_string();
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

#[command("roles")]
#[required_permissions(Restricted)]
#[allow_slash(false)]
#[group("Settings")]
#[description("Change the roles allowed to use the bot")]
pub async fn set_allowed_roles(
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

    if args.is_empty() {
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

#[command("greet")]
#[group("Settings")]
#[description("Set a join sound")]
#[arg(
    name = "query",
    kind = "String",
    description = "Name or ID of sound to set as your greet sound",
    required = false
)]
pub async fn set_greet_sound(
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

    let query = args
        .named("query")
        .map(|s| s.to_owned())
        .unwrap_or(String::new());
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
            &query,
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
#[group("Settings")]
#[description("Configure whether users should be able to use join sounds")]
#[required_permissions(Restricted)]
pub async fn allow_greet_sounds(
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
