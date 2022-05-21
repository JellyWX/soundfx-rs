use poise::serenity_prelude::{Attachment, GuildId, RoleId};
use tokio::fs::File;

use crate::{
    cmds::autocomplete_sound,
    consts::{MAX_SOUNDS, PATREON_GUILD, PATREON_ROLE},
    models::sound::{Sound, SoundCtx},
    Context, Error,
};

/// Upload a new sound to the bot
#[poise::command(
    slash_command,
    rename = "upload",
    category = "Manage",
    default_member_permissions = "MANAGE_GUILD"
)]
pub async fn upload_new_sound(
    ctx: Context<'_>,
    #[description = "Name to upload sound to"] name: String,
    #[description = "Sound file (max. 2MB)"] file: Attachment,
) -> Result<(), Error> {
    ctx.defer().await?;

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

    if !name.is_empty() && name.len() <= 20 {
        if !is_numeric(&name) {
            // need to check the name is not currently in use by the user
            let count_name =
                Sound::count_named_user_sounds(ctx.author().id, &name, &ctx.data().database)
                    .await?;
            if count_name > 0 {
                ctx.say(
                    "You are already using that name. Please choose a unique name for your upload.",
                )
                .await?;
            } else {
                // need to check how many sounds user currently has
                let count = Sound::count_user_sounds(ctx.author().id, &ctx.data().database).await?;
                let mut permit_upload = true;

                // need to check if user is patreon or nah
                if count >= *MAX_SOUNDS {
                    let patreon_guild_member = GuildId(*PATREON_GUILD)
                        .member(ctx.discord(), ctx.author().id)
                        .await;

                    if let Ok(member) = patreon_guild_member {
                        permit_upload = member.roles.contains(&RoleId(*PATREON_ROLE));
                    } else {
                        permit_upload = false;
                    }
                }

                if permit_upload {
                    match Sound::create_anon(
                        &name,
                        file.url.as_str(),
                        ctx.guild_id().unwrap(),
                        ctx.author().id,
                        &ctx.data().database,
                    )
                    .await
                    {
                        Ok(_) => {
                            ctx.say("Sound has been uploaded").await?;
                        }

                        Err(e) => {
                            println!("Error occurred during upload: {:?}", e);
                            ctx.say("Sound failed to upload.").await?;
                        }
                    }
                } else {
                    ctx.say(format!(
                            "You have reached the maximum number of sounds ({}). Either delete some with `/delete` or join our Patreon for unlimited uploads at **https://patreon.com/jellywx**",
                            *MAX_SOUNDS,
                        )).await?;
                }
            }
        } else {
            ctx.say("Please ensure the sound name contains a non-numerical character")
                .await?;
        }
    } else {
        ctx.say("Usage: `/upload <name>`. Please ensure the name provided is less than 20 characters in length").await?;
    }

    Ok(())
}

/// Delete a sound you have uploaded
#[poise::command(slash_command, rename = "delete", category = "Manage")]
pub async fn delete_sound(
    ctx: Context<'_>,
    #[description = "Name or ID of sound to delete"]
    #[autocomplete = "autocomplete_sound"]
    name: String,
) -> Result<(), Error> {
    let pool = ctx.data().database.clone();

    let uid = ctx.author().id.0;
    let gid = ctx.guild_id().unwrap().0;

    let sound_vec = ctx.data().search_for_sound(&name, gid, uid, true).await?;
    let sound_result = sound_vec.first();

    match sound_result {
        Some(sound) => {
            if sound.uploader_id != Some(uid) && sound.server_id != gid {
                ctx.say("You can only delete sounds from this guild or that you have uploaded.")
                    .await?;
            } else {
                let has_perms = {
                    if let Ok(member) = ctx.guild_id().unwrap().member(&ctx.discord(), uid).await {
                        if let Ok(perms) = member.permissions(&ctx.discord()) {
                            perms.manage_guild()
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                if sound.uploader_id == Some(uid) || has_perms {
                    sound.delete(&pool).await?;

                    ctx.say("Sound has been deleted").await?;
                } else {
                    ctx.say("Only server admins can delete sounds uploaded by other users.")
                        .await?;
                }
            }
        }

        None => {
            ctx.say("Sound could not be found by that name.").await?;
        }
    }

    Ok(())
}

/// Change a sound between public and private
#[poise::command(slash_command, rename = "public", category = "Manage")]
pub async fn change_public(
    ctx: Context<'_>,
    #[description = "Name or ID of sound to change privacy setting of"]
    #[autocomplete = "autocomplete_sound"]
    name: String,
) -> Result<(), Error> {
    let pool = ctx.data().database.clone();

    let uid = ctx.author().id.0;
    let gid = ctx.guild_id().unwrap().0;

    let mut sound_vec = ctx.data().search_for_sound(&name, gid, uid, true).await?;
    let sound_result = sound_vec.first_mut();

    match sound_result {
        Some(sound) => {
            if sound.uploader_id != Some(uid) {
                ctx.say("You can only change the visibility of sounds you have uploaded. Use `/list` to view your sounds").await?;
            } else {
                if sound.public {
                    sound.public = false;

                    ctx.say("Sound has been set to private 🔒").await?;
                } else {
                    sound.public = true;

                    ctx.say("Sound has been set to public 🔓").await?;
                }

                sound.commit(&pool).await?
            }
        }

        None => {
            ctx.say("Sound could not be found by that name.").await?;
        }
    }

    Ok(())
}

/// Download a sound file from the bot
#[poise::command(slash_command, rename = "download", category = "Manage")]
pub async fn download_file(
    ctx: Context<'_>,
    #[description = "Name or ID of sound to download"]
    #[autocomplete = "autocomplete_sound"]
    name: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let sound = ctx
        .data()
        .search_for_sound(&name, ctx.guild_id().unwrap(), ctx.author().id, true)
        .await?;

    match sound.first() {
        Some(sound) => {
            let source = sound.store_sound_source(&ctx.data().database).await?;

            let file = File::open(&source).await?;
            let name = format!("{}-{}.opus", sound.id, sound.name);

            ctx.send(|m| m.attachment((&file, name.as_str()).into()))
                .await?;
        }

        None => {
            ctx.say("No sound found by specified name/ID").await?;
        }
    }

    Ok(())
}
