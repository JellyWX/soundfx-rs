use poise::{serenity::constants::MESSAGE_CODE_LIMIT, CreateReply};

use crate::{
    models::sound::{Sound, SoundCtx},
    Context, Error,
};

fn format_search_results<'a>(search_results: Vec<Sound>) -> CreateReply<'a> {
    let mut builder = CreateReply::default();

    let mut current_character_count = 0;
    let title = "Public sounds matching filter:";

    let field_iter = search_results
        .iter()
        .take(25)
        .map(|item| (&item.name, format!("ID: {}", item.id), true))
        .filter(|item| {
            current_character_count += item.0.len() + item.1.len();

            current_character_count <= MESSAGE_CODE_LIMIT - title.len()
        });

    builder.embed(|e| e.title(title).fields(field_iter));

    builder
}

/// Show the sounds uploaded to this server
#[poise::command(slash_command, rename = "list")]
pub async fn list_sounds(ctx: Context<'_>) -> Result<(), Error> {
    let sounds;
    let mut message_buffer;

    sounds = ctx.data().guild_sounds(ctx.guild_id().unwrap()).await?;

    message_buffer = "Sounds on this server: ".to_string();

    // todo change this to iterator
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
            ctx.say(message_buffer).await?;

            message_buffer = "".to_string();
        }
    }

    if message_buffer.len() > 0 {
        ctx.say(message_buffer).await?;
    }

    Ok(())
}

/// Show all sounds you have uploaded
#[poise::command(slash_command, rename = "me")]
pub async fn list_user_sounds(ctx: Context<'_>) -> Result<(), Error> {
    let sounds;
    let mut message_buffer;

    sounds = ctx.data().user_sounds(ctx.author().id).await?;

    message_buffer = "Sounds on this server: ".to_string();

    // todo change this to iterator
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
            ctx.say(message_buffer).await?;

            message_buffer = "".to_string();
        }
    }

    if message_buffer.len() > 0 {
        ctx.say(message_buffer).await?;
    }

    Ok(())
}

/// Search for sounds
#[poise::command(slash_command, rename = "search", category = "Search")]
pub async fn search_sounds(
    ctx: Context<'_>,
    #[description = "Sound name to search for"] query: String,
) -> Result<(), Error> {
    let search_results = ctx
        .data()
        .search_for_sound(&query, ctx.guild_id().unwrap(), ctx.author().id, false)
        .await?;

    ctx.send(|m| {
        *m = format_search_results(search_results);
        m
    })
    .await?;

    Ok(())
}

/// Show a page of random sounds
#[poise::command(slash_command, rename = "random")]
pub async fn show_random_sounds(ctx: Context<'_>) -> Result<(), Error> {
    let search_results = sqlx::query_as_unchecked!(
        Sound,
        "
SELECT name, id, public, server_id, uploader_id
    FROM sounds
    WHERE public = 1
    ORDER BY rand()
    LIMIT 25
        "
    )
    .fetch_all(&ctx.data().database)
    .await?;

    ctx.send(|m| {
        *m = format_search_results(search_results);
        m
    })
    .await?;

    Ok(())
}
