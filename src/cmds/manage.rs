use std::time::Duration;

use regex_command_attr::command;
use serenity::{
    client::Context,
    framework::standard::CommandResult,
    model::id::{GuildId, RoleId},
};

use crate::{
    framework::{Args, CommandInvoke, CreateGenericResponse},
    sound::Sound,
    MySQL, MAX_SOUNDS, PATREON_GUILD, PATREON_ROLE,
};

#[command("upload")]
#[group("Manage")]
#[description("Upload a new sound to the bot")]
#[arg(
    name = "name",
    description = "Name to upload sound to",
    kind = "String",
    required = true
)]
pub async fn upload_new_sound(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
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

    let new_name = args
        .named("name")
        .map(|n| n.to_string())
        .unwrap_or(String::new());

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
                Sound::count_named_user_sounds(invoke.author_id().0, &new_name, pool.clone())
                    .await?;
            if count_name > 0 {
                invoke.respond(ctx.http.clone(), CreateGenericResponse::new().content("You are already using that name. Please choose a unique name for your upload.")).await?;
            } else {
                // need to check how many sounds user currently has
                let count = Sound::count_user_sounds(invoke.author_id().0, pool.clone()).await?;
                let mut permit_upload = true;

                // need to check if user is patreon or nah
                if count >= *MAX_SOUNDS {
                    let patreon_guild_member = GuildId(*PATREON_GUILD)
                        .member(ctx, invoke.author_id())
                        .await;

                    if let Ok(member) = patreon_guild_member {
                        permit_upload = member.roles.contains(&RoleId(*PATREON_ROLE));
                    } else {
                        permit_upload = false;
                    }
                }

                if permit_upload {
                    let attachment = if let Some(attachment) = invoke
                        .msg()
                        .map(|m| m.attachments.get(0).map(|a| a.url.clone()))
                        .flatten()
                    {
                        Some(attachment)
                    } else {
                        invoke.respond(ctx.http.clone(), CreateGenericResponse::new().content("Please now upload an audio file under 1MB in size (larger files will be automatically trimmed):")).await?;

                        let reply = invoke
                            .channel_id()
                            .await_reply(&ctx)
                            .author_id(invoke.author_id())
                            .timeout(Duration::from_secs(120))
                            .await;

                        match reply {
                            Some(reply_msg) => {
                                if let Some(attachment) = reply_msg.attachments.get(0) {
                                    Some(attachment.url.clone())
                                } else {
                                    invoke.followup(ctx.http.clone(), CreateGenericResponse::new().content("Please upload 1 attachment following your upload command. Aborted")).await?;

                                    None
                                }
                            }

                            None => {
                                invoke
                                    .followup(
                                        ctx.http.clone(),
                                        CreateGenericResponse::new()
                                            .content("Upload timed out. Please redo the command"),
                                    )
                                    .await?;

                                None
                            }
                        }
                    };

                    if let Some(url) = attachment {
                        match Sound::create_anon(
                            &new_name,
                            url.as_str(),
                            invoke.guild_id().unwrap().0,
                            invoke.author_id().0,
                            pool,
                        )
                        .await
                        {
                            Ok(_) => {
                                invoke
                                    .followup(
                                        ctx.http.clone(),
                                        CreateGenericResponse::new()
                                            .content("Sound has been uploaded"),
                                    )
                                    .await?;
                            }

                            Err(e) => {
                                println!("Error occurred during upload: {:?}", e);
                                invoke
                                    .followup(
                                        ctx.http.clone(),
                                        CreateGenericResponse::new()
                                            .content("Sound failed to upload."),
                                    )
                                    .await?;
                            }
                        }
                    }
                } else {
                    invoke.respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new().content(format!(
                            "You have reached the maximum number of sounds ({}). Either delete some with `/delete` or join our Patreon for unlimited uploads at **https://patreon.com/jellywx**",
                            *MAX_SOUNDS,
                        ))).await?;
                }
            }
        } else {
            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new()
                        .content("Please ensure the sound name contains a non-numerical character"),
                )
                .await?;
        }
    } else {
        invoke.respond(ctx.http.clone(), CreateGenericResponse::new().content("Usage: `/upload <name>`. Please ensure the name provided is less than 20 characters in length")).await?;
    }

    Ok(())
}

#[command("delete")]
#[group("Manage")]
#[description("Delete a sound you have uploaded")]
#[arg(
    name = "query",
    description = "Delete sound with the specified name or ID",
    kind = "String",
    required = true
)]
#[example("`/delete beep` - delete the sound with the name \"beep\"")]
pub async fn delete_sound(
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

    let uid = invoke.author_id().0;
    let gid = invoke.guild_id().unwrap().0;

    let name = args
        .named("query")
        .map(|s| s.to_owned())
        .unwrap_or(String::new());

    let sound_vec = Sound::search_for_sound(&name, gid, uid, pool.clone(), true).await?;
    let sound_result = sound_vec.first();

    match sound_result {
        Some(sound) => {
            if sound.uploader_id != Some(uid) && sound.server_id != gid {
                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new().content(
                            "You can only delete sounds from this guild or that you have uploaded.",
                        ),
                    )
                    .await?;
            } else {
                let has_perms = {
                    if let Ok(member) = invoke.member(&ctx).await {
                        if let Ok(perms) = member.permissions(&ctx) {
                            perms.manage_guild()
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                if sound.uploader_id == Some(uid) || has_perms {
                    sound.delete(pool).await?;

                    invoke
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new().content("Sound has been deleted"),
                        )
                        .await?;
                } else {
                    invoke
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new().content(
                                "Only server admins can delete sounds uploaded by other users.",
                            ),
                        )
                        .await?;
                }
            }
        }

        None => {
            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().content("Sound could not be found by that name."),
                )
                .await?;
        }
    }

    Ok(())
}

#[command("public")]
#[group("Manage")]
#[description("Change a sound between public and private")]
#[arg(
    name = "query",
    kind = "String",
    description = "Sound name or ID to change the privacy setting of",
    required = true
)]
#[example("`/public 12` - change sound with ID 12 to private")]
#[example("`/public 12` - change sound with ID 12 back to public")]
pub async fn change_public(
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

    let uid = invoke.author_id().as_u64().to_owned();

    let name = args.named("query").unwrap();
    let gid = *invoke.guild_id().unwrap().as_u64();

    let mut sound_vec = Sound::search_for_sound(name, gid, uid, pool.clone(), true).await?;
    let sound_result = sound_vec.first_mut();

    match sound_result {
        Some(sound) => {
            if sound.uploader_id != Some(uid) {
                invoke.respond(ctx.http.clone(), CreateGenericResponse::new().content("You can only change the visibility of sounds you have uploaded. Use `?list me` to view your sounds")).await?;
            } else {
                if sound.public {
                    sound.public = false;

                    invoke
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new()
                                .content("Sound has been set to private 🔒"),
                        )
                        .await?;
                } else {
                    sound.public = true;

                    invoke
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new().content("Sound has been set to public 🔓"),
                        )
                        .await?;
                }

                sound.commit(pool).await?
            }
        }

        None => {
            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().content("Sound could not be found by that name."),
                )
                .await?;
        }
    }

    Ok(())
}
