use regex_command_attr::command;
use serenity::{client::Context, framework::standard::CommandResult};
use songbird;

use crate::framework::{Args, CommandInvoke, CreateGenericResponse};

#[command("stop")]
#[required_permissions(Managed)]
#[group("Stop")]
#[description("Stop the bot from playing")]
pub async fn stop_playing(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    _args: Args,
) -> CommandResult {
    let guild_id = invoke.guild_id().unwrap();

    let songbird = songbird::get(ctx).await.unwrap();
    let call_opt = songbird.get(guild_id);

    if let Some(call) = call_opt {
        let mut lock = call.lock().await;

        lock.stop();
    }

    invoke
        .respond(ctx.http.clone(), CreateGenericResponse::new().content("👍"))
        .await?;

    Ok(())
}

#[command]
#[aliases("dc")]
#[required_permissions(Managed)]
#[group("Stop")]
#[description("Disconnect the bot")]
pub async fn disconnect(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    _args: Args,
) -> CommandResult {
    let guild_id = invoke.guild_id().unwrap();

    let songbird = songbird::get(ctx).await.unwrap();
    let _ = songbird.leave(guild_id).await;

    invoke
        .respond(ctx.http.clone(), CreateGenericResponse::new().content("👍"))
        .await?;

    Ok(())
}
