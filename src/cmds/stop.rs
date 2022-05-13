use songbird;

use crate::{Context, Error};

/// Stop the bot from playing and clear the play queue
#[poise::command(slash_command, rename = "stop", default_member_permissions = "SPEAK")]
pub async fn stop_playing(ctx: Context<'_>) -> Result<(), Error> {
    let songbird = songbird::get(ctx.discord()).await.unwrap();
    let call_opt = songbird.get(ctx.guild_id().unwrap());

    if let Some(call) = call_opt {
        let mut lock = call.lock().await;

        lock.stop();
    }

    ctx.say("👍").await?;

    Ok(())
}

/// Disconnect the bot
#[poise::command(slash_command, default_member_permissions = "SPEAK")]
pub async fn disconnect(ctx: Context<'_>) -> Result<(), Error> {
    let songbird = songbird::get(ctx.discord()).await.unwrap();
    let _ = songbird.leave(ctx.guild_id().unwrap()).await;

    ctx.say("👍").await?;

    Ok(())
}
