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
#[example("`/volume` - check the volume on the current server")]
#[example("`/volume 100` - reset the volume on the current server")]
#[example("`/volume 10` - set the volume on the current server to 10%")]
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
#[kind(Text)]
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
        if prefix.len() <= 5 && !prefix.is_empty() {
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
#[group("Settings")]
#[description("Change the role allowed to use the bot")]
#[arg(
    name = "role",
    kind = "Role",
    description = "A role to allow to use the bot. Use @everyone to allow all server members",
    required = true
)]
#[example("`/roles @everyone` - allow all server members to use the bot")]
#[example("`/roles @DJ` - allow only server members with the 'DJ' role to use the bot")]
pub async fn set_allowed_roles(
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

    let role_id = args.named("role").unwrap().parse::<u64>().unwrap();
    let guild_data = ctx.guild_data(invoke.guild_id().unwrap()).await.unwrap();

    guild_data.write().await.allowed_role = Some(role_id);
    guild_data.read().await.commit(pool).await?;

    invoke
        .respond(
            ctx.http.clone(),
            CreateGenericResponse::new().content(format!("Allowed role set to <@&{}>", role_id)),
        )
        .await?;

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
#[example("`/greet` - remove your join sound")]
#[example("`/greet 1523` - set your join sound to sound with ID 1523")]
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
#[example("`/allow_greet` - disable greet sounds in the server")]
#[example("`/allow_greet` - re-enable greet sounds in the server")]
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
