use crate::{
    models::{guild_data::CtxGuildData, join_sound::JoinSoundCtx, sound::SoundCtx},
    Context, Error,
};

/// Change the bot's volume in this server
#[poise::command(slash_command, rename = "volume", guild_only = true)]
pub async fn change_volume(
    ctx: Context<'_>,
    #[description = "New volume as a percentage"] volume: Option<usize>,
) -> Result<(), Error> {
    let guild_data_opt = ctx.guild_data(ctx.guild_id().unwrap()).await;
    let guild_data = guild_data_opt.unwrap();

    if let Some(volume) = volume {
        guild_data.write().await.volume = volume as u8;

        guild_data.read().await.commit(&ctx.data().database).await?;

        ctx.say(format!("Volume changed to {}%", volume)).await?;
    } else {
        let read = guild_data.read().await;

        ctx.say(format!(
            "Current server volume: {vol}%. Change the volume with `/volume <new volume>`",
            vol = read.volume
        ))
        .await?;
    }

    Ok(())
}

/// Manage greet sounds on this server
#[poise::command(slash_command, rename = "greet", guild_only = true)]
pub async fn greet_sound(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Set a join sound
#[poise::command(slash_command, rename = "set")]
pub async fn set_greet_sound(
    ctx: Context<'_>,
    #[description = "Name or ID of sound to set as your join sound"] name: String,
) -> Result<(), Error> {
    let sound_vec = ctx
        .data()
        .search_for_sound(&name, ctx.guild_id().unwrap(), ctx.author().id, true)
        .await?;

    match sound_vec.first() {
        Some(sound) => {
            ctx.data()
                .update_join_sound(ctx.author().id, Some(sound.id))
                .await;

            ctx.say(format!(
                "Greet sound has been set to {} (ID {})",
                sound.name, sound.id
            ))
            .await?;
        }

        None => {
            ctx.say("Could not find a sound by that name.").await?;
        }
    }

    Ok(())
}

/// Set a join sound
#[poise::command(slash_command, rename = "unset", guild_only = true)]
pub async fn unset_greet_sound(ctx: Context<'_>) -> Result<(), Error> {
    ctx.data().update_join_sound(ctx.author().id, None).await;

    ctx.say("Greet sound has been unset").await?;

    Ok(())
}

/// Disable greet sounds on this server
#[poise::command(slash_command, rename = "disable", guild_only = true)]
pub async fn disable_greet_sound(ctx: Context<'_>) -> Result<(), Error> {
    let guild_data_opt = ctx.guild_data(ctx.guild_id().unwrap()).await;

    if let Ok(guild_data) = guild_data_opt {
        guild_data.write().await.allow_greets = false;

        guild_data.read().await.commit(&ctx.data().database).await?;
    }

    ctx.say("Greet sounds have been disabled in this server")
        .await?;

    Ok(())
}

/// Enable greet sounds on this server
#[poise::command(slash_command, rename = "enable", guild_only = true)]
pub async fn enable_greet_sound(ctx: Context<'_>) -> Result<(), Error> {
    let guild_data_opt = ctx.guild_data(ctx.guild_id().unwrap()).await;

    if let Ok(guild_data) = guild_data_opt {
        guild_data.write().await.allow_greets = true;

        guild_data.read().await.commit(&ctx.data().database).await?;
    }

    ctx.say("Greet sounds have been enable in this server")
        .await?;

    Ok(())
}
