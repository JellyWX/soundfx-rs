use crate::sound::Sound;

fn format_search_results(search_results: Vec<Sound>) -> CreateGenericResponse {
    let mut current_character_count = 0;
    let title = "Public sounds matching filter:";

    let field_iter = search_results
        .iter()
        .take(25)
        .map(|item| (&item.name, format!("ID: {}", item.id), true))
        .filter(|item| {
            current_character_count += item.0.len() + item.1.len();

            current_character_count <= serenity::constants::MESSAGE_CODE_LIMIT - title.len()
        });

    CreateGenericResponse::new().embed(|e| e.title(title).fields(field_iter))
}

#[command("list")]
#[group("Search")]
#[description("Show the sounds uploaded by you or to your server")]
#[arg(
    name = "me",
    description = "Whether to list your sounds or server sounds (default: server)",
    kind = "Boolean",
    required = false
)]
#[example("`/list` - list sounds uploaded to the server you're in")]
#[example("`/list [me: True]` - list sounds you have uploaded across all servers")]
pub async fn list_sounds(
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

    let sounds;
    let mut message_buffer;

    if args.named("me").map(|i| i.to_owned()) == Some("me".to_string()) {
        sounds = Sound::user_sounds(invoke.author_id(), pool).await?;

        message_buffer = "All your sounds: ".to_string();
    } else {
        sounds = Sound::guild_sounds(invoke.guild_id().unwrap(), pool).await?;

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
            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().content(message_buffer),
                )
                .await?;

            message_buffer = "".to_string();
        }
    }

    if message_buffer.len() > 0 {
        invoke
            .respond(
                ctx.http.clone(),
                CreateGenericResponse::new().content(message_buffer),
            )
            .await?;
    }

    Ok(())
}

#[command("search")]
#[group("Search")]
#[description("Search for sounds")]
#[arg(
    name = "query",
    kind = "String",
    description = "Sound name to search for",
    required = true
)]
pub async fn search_sounds(
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

    let query = args.named("query").unwrap();

    let search_results = Sound::search_for_sound(
        query,
        invoke.guild_id().unwrap(),
        invoke.author_id(),
        pool,
        false,
    )
    .await?;

    invoke
        .respond(ctx.http.clone(), format_search_results(search_results))
        .await?;

    Ok(())
}

#[command("random")]
#[group("Search")]
#[description("Show a page of random sounds")]
pub async fn show_random_sounds(
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
        .expect("Could not get SQLPool from data");

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
    .fetch_all(&pool)
    .await
    .unwrap();

    invoke
        .respond(ctx.http.clone(), format_search_results(search_results))
        .await?;

    Ok(())
}
