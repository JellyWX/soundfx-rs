use regex_command_attr::command;

use serenity::{
    builder::CreateActionRow,
    client::Context,
    framework::standard::CommandResult,
    model::interactions::{ButtonStyle, InteractionResponseType},
};

use songbird::{
    create_player, ffmpeg,
    input::{cached::Memory, Input},
    Event,
};

use crate::{
    event_handlers::RestartTrack,
    framework::{Args, CommandInvoke, CreateGenericResponse},
    guild_data::CtxGuildData,
    join_channel, play_from_query,
    sound::Sound,
    AudioIndex, MySQL,
};

use std::{convert::TryFrom, time::Duration};

#[command]
#[aliases("p")]
#[required_permissions(Managed)]
#[group("Play")]
#[description("Play a sound in your current voice channel")]
#[arg(
    name = "query",
    description = "Play sound with the specified name or ID",
    kind = "String",
    required = true
)]
#[example("`/play ubercharge` - play sound with name \"ubercharge\" ")]
#[example("`/play 13002` - play sound with ID 13002")]
pub async fn play(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let guild = invoke.guild(ctx.cache.clone()).await.unwrap();

    invoke
        .respond(
            ctx.http.clone(),
            CreateGenericResponse::new()
                .content(play_from_query(ctx, guild, invoke.author_id(), args, false).await),
        )
        .await?;

    Ok(())
}

#[command("loop")]
#[required_permissions(Managed)]
#[group("Play")]
#[description("Play a sound on loop in your current voice channel")]
#[arg(
    name = "query",
    description = "Play sound with the specified name or ID",
    kind = "String",
    required = true
)]
#[example("`/loop rain` - loop sound with name \"rain\" ")]
#[example("`/loop 13002` - play sound with ID 13002")]
pub async fn loop_play(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let guild = invoke.guild(ctx.cache.clone()).await.unwrap();

    invoke
        .respond(
            ctx.http.clone(),
            CreateGenericResponse::new()
                .content(play_from_query(ctx, guild, invoke.author_id(), args, true).await),
        )
        .await?;

    Ok(())
}

#[command("ambience")]
#[required_permissions(Managed)]
#[group("Play")]
#[description("Play ambient sound in your current voice channel")]
#[arg(
    name = "name",
    description = "Play sound with the specified name",
    kind = "String",
    required = false
)]
#[example("`/ambience rain on tent` - play the ambient sound \"rain on tent\" ")]
pub async fn play_ambience(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    let guild = invoke.guild(ctx.cache.clone()).await.unwrap();

    let channel_to_join = guild
        .voice_states
        .get(&invoke.author_id())
        .and_then(|voice_state| voice_state.channel_id);

    match channel_to_join {
        Some(user_channel) => {
            let search_name = args.named("name").unwrap().to_lowercase();
            let audio_index = ctx.data.read().await.get::<AudioIndex>().cloned().unwrap();

            if let Some(filename) = audio_index.get(&search_name) {
                let (track, track_handler) = create_player(
                    Input::try_from(
                        Memory::new(ffmpeg(format!("audio/{}", filename)).await.unwrap()).unwrap(),
                    )
                    .unwrap(),
                );

                let (call_handler, _) = join_channel(ctx, guild.clone(), user_channel).await;
                let guild_data = ctx.guild_data(guild).await.unwrap();

                {
                    let mut lock = call_handler.lock().await;

                    lock.play(track);
                }

                let _ = track_handler.set_volume(guild_data.read().await.volume as f32 / 100.0);
                let _ = track_handler.add_event(
                    Event::Periodic(
                        track_handler.metadata().duration.unwrap() - Duration::from_millis(200),
                        None,
                    ),
                    RestartTrack {},
                );

                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new()
                            .content(format!("Playing ambience **{}**", search_name)),
                    )
                    .await?;
            } else {
                invoke
                    .respond(
                        ctx.http.clone(),
                        CreateGenericResponse::new().embed(|e| {
                            e.title("Not Found").description(format!(
                                "Could not find ambience sound by name **{}**

__Available ambience sounds:__
{}",
                                search_name,
                                audio_index
                                    .keys()
                                    .into_iter()
                                    .map(|i| i.as_str())
                                    .collect::<Vec<&str>>()
                                    .join("\n")
                            ))
                        }),
                    )
                    .await?;
            }
        }

        None => {
            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().content("You are not in a voice chat!"),
                )
                .await?;
        }
    }

    Ok(())
}

#[command("soundboard")]
#[required_permissions(Managed)]
#[group("Play")]
#[kind(Slash)]
#[description("Get a menu of sounds with buttons to play them")]
#[arg(
    name = "1",
    description = "Query for sound button 1",
    kind = "String",
    required = true
)]
#[arg(
    name = "2",
    description = "Query for sound button 2",
    kind = "String",
    required = false
)]
#[arg(
    name = "3",
    description = "Query for sound button 3",
    kind = "String",
    required = false
)]
#[arg(
    name = "4",
    description = "Query for sound button 4",
    kind = "String",
    required = false
)]
#[arg(
    name = "5",
    description = "Query for sound button 5",
    kind = "String",
    required = false
)]
#[arg(
    name = "6",
    description = "Query for sound button 6",
    kind = "String",
    required = false
)]
#[arg(
    name = "7",
    description = "Query for sound button 7",
    kind = "String",
    required = false
)]
#[arg(
    name = "8",
    description = "Query for sound button 8",
    kind = "String",
    required = false
)]
#[arg(
    name = "9",
    description = "Query for sound button 9",
    kind = "String",
    required = false
)]
#[arg(
    name = "10",
    description = "Query for sound button 10",
    kind = "String",
    required = false
)]
#[arg(
    name = "11",
    description = "Query for sound button 11",
    kind = "String",
    required = false
)]
#[arg(
    name = "12",
    description = "Query for sound button 12",
    kind = "String",
    required = false
)]
#[arg(
    name = "13",
    description = "Query for sound button 13",
    kind = "String",
    required = false
)]
#[arg(
    name = "14",
    description = "Query for sound button 14",
    kind = "String",
    required = false
)]
#[arg(
    name = "15",
    description = "Query for sound button 15",
    kind = "String",
    required = false
)]
#[arg(
    name = "16",
    description = "Query for sound button 16",
    kind = "String",
    required = false
)]
#[arg(
    name = "17",
    description = "Query for sound button 17",
    kind = "String",
    required = false
)]
#[arg(
    name = "18",
    description = "Query for sound button 18",
    kind = "String",
    required = false
)]
#[arg(
    name = "19",
    description = "Query for sound button 19",
    kind = "String",
    required = false
)]
#[arg(
    name = "20",
    description = "Query for sound button 20",
    kind = "String",
    required = false
)]
#[arg(
    name = "21",
    description = "Query for sound button 21",
    kind = "String",
    required = false
)]
#[arg(
    name = "22",
    description = "Query for sound button 22",
    kind = "String",
    required = false
)]
#[arg(
    name = "23",
    description = "Query for sound button 23",
    kind = "String",
    required = false
)]
#[arg(
    name = "24",
    description = "Query for sound button 24",
    kind = "String",
    required = false
)]
#[arg(
    name = "25",
    description = "Query for sound button 25",
    kind = "String",
    required = false
)]
#[example("`/soundboard ubercharge` - create a soundboard with a button for the \"ubercharge\" sound effect")]
#[example("`/soundboard 57000 24119 2 1002 13202` - create a soundboard with 5 buttons, for sounds with the IDs presented")]
pub async fn soundboard(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    if let Some(interaction) = invoke.interaction() {
        let _ = interaction
            .create_interaction_response(&ctx, |r| {
                r.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await;
    }

    let pool = ctx
        .data
        .read()
        .await
        .get::<MySQL>()
        .cloned()
        .expect("Could not get SQLPool from data");

    let mut sounds = vec![];

    for n in 1..25 {
        let search = Sound::search_for_sound(
            args.named(&n.to_string()).unwrap_or(&"".to_string()),
            invoke.guild_id().unwrap(),
            invoke.author_id(),
            pool.clone(),
            true,
        )
        .await?;

        if let Some(sound) = search.first() {
            sounds.push(sound.clone());
        }
    }

    invoke
        .followup(
            ctx.http.clone(),
            CreateGenericResponse::new()
                .content("**Play a sound:**")
                .components(|c| {
                    for row in sounds.as_slice().chunks(5) {
                        let mut action_row: CreateActionRow = Default::default();
                        for sound in row {
                            action_row.create_button(|b| {
                                b.style(ButtonStyle::Primary)
                                    .label(&sound.name)
                                    .custom_id(sound.id)
                            });
                        }

                        c.add_action_row(action_row);
                    }

                    c
                }),
        )
        .await?;

    Ok(())
}
