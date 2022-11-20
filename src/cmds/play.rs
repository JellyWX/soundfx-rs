use poise::{
    serenity::{builder::CreateActionRow, model::application::component::ButtonStyle},
    serenity_prelude::GuildChannel,
};

use crate::{
    cmds::autocomplete_sound,
    models::{guild_data::CtxGuildData, sound::SoundCtx},
    utils::{join_channel, play_from_query, queue_audio},
    Context, Error,
};

/// Play a sound in your current voice channel
#[poise::command(slash_command, default_member_permissions = "SPEAK", guild_only = true)]
pub async fn play(
    ctx: Context<'_>,
    #[description = "Name or ID of sound to play"]
    #[autocomplete = "autocomplete_sound"]
    name: String,
    #[description = "Channel to play in (default: your current voice channel)"] channel: Option<
        GuildChannel,
    >,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild = ctx.guild().unwrap();

    if channel.as_ref().map_or(false, |c| c.is_text_based()) {
        ctx.say("The channel specified is not a voice channel.")
            .await?;
    } else {
        ctx.say(
            play_from_query(
                &ctx.discord(),
                &ctx.data(),
                guild,
                ctx.author().id,
                channel.map(|c| c.id),
                &name,
                false,
            )
            .await,
        )
        .await?;
    }

    Ok(())
}

/// Play up to 25 sounds on queue
#[poise::command(
    slash_command,
    rename = "queue",
    default_member_permissions = "SPEAK",
    guild_only = true
)]
pub async fn queue_play(
    ctx: Context<'_>,
    #[description = "Name or ID for queue position 1"]
    #[autocomplete = "autocomplete_sound"]
    sound_1: String,
    #[description = "Name or ID for queue position 2"]
    #[autocomplete = "autocomplete_sound"]
    sound_2: String,
    #[description = "Name or ID for queue position 3"]
    #[autocomplete = "autocomplete_sound"]
    sound_3: Option<String>,
    #[description = "Name or ID for queue position 4"]
    #[autocomplete = "autocomplete_sound"]
    sound_4: Option<String>,
    #[description = "Name or ID for queue position 5"]
    #[autocomplete = "autocomplete_sound"]
    sound_5: Option<String>,
    #[description = "Name or ID for queue position 6"]
    #[autocomplete = "autocomplete_sound"]
    sound_6: Option<String>,
    #[description = "Name or ID for queue position 7"]
    #[autocomplete = "autocomplete_sound"]
    sound_7: Option<String>,
    #[description = "Name or ID for queue position 8"]
    #[autocomplete = "autocomplete_sound"]
    sound_8: Option<String>,
    #[description = "Name or ID for queue position 9"]
    #[autocomplete = "autocomplete_sound"]
    sound_9: Option<String>,
    #[description = "Name or ID for queue position 10"]
    #[autocomplete = "autocomplete_sound"]
    sound_10: Option<String>,
    #[description = "Name or ID for queue position 11"]
    #[autocomplete = "autocomplete_sound"]
    sound_11: Option<String>,
    #[description = "Name or ID for queue position 12"]
    #[autocomplete = "autocomplete_sound"]
    sound_12: Option<String>,
    #[description = "Name or ID for queue position 13"]
    #[autocomplete = "autocomplete_sound"]
    sound_13: Option<String>,
    #[description = "Name or ID for queue position 14"]
    #[autocomplete = "autocomplete_sound"]
    sound_14: Option<String>,
    #[description = "Name or ID for queue position 15"]
    #[autocomplete = "autocomplete_sound"]
    sound_15: Option<String>,
    #[description = "Name or ID for queue position 16"]
    #[autocomplete = "autocomplete_sound"]
    sound_16: Option<String>,
    #[description = "Name or ID for queue position 17"]
    #[autocomplete = "autocomplete_sound"]
    sound_17: Option<String>,
    #[description = "Name or ID for queue position 18"]
    #[autocomplete = "autocomplete_sound"]
    sound_18: Option<String>,
    #[description = "Name or ID for queue position 19"]
    #[autocomplete = "autocomplete_sound"]
    sound_19: Option<String>,
    #[description = "Name or ID for queue position 20"]
    #[autocomplete = "autocomplete_sound"]
    sound_20: Option<String>,
    #[description = "Name or ID for queue position 21"]
    #[autocomplete = "autocomplete_sound"]
    sound_21: Option<String>,
    #[description = "Name or ID for queue position 22"]
    #[autocomplete = "autocomplete_sound"]
    sound_22: Option<String>,
    #[description = "Name or ID for queue position 23"]
    #[autocomplete = "autocomplete_sound"]
    sound_23: Option<String>,
    #[description = "Name or ID for queue position 24"]
    #[autocomplete = "autocomplete_sound"]
    sound_24: Option<String>,
    #[description = "Name or ID for queue position 25"]
    #[autocomplete = "autocomplete_sound"]
    sound_25: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild = ctx.guild().unwrap();

    let channel_to_join = guild
        .voice_states
        .get(&ctx.author().id)
        .and_then(|voice_state| voice_state.channel_id);

    match channel_to_join {
        Some(user_channel) => {
            let (call_handler, _) = join_channel(ctx.discord(), guild.clone(), user_channel).await;

            let guild_data = ctx
                .data()
                .guild_data(ctx.guild_id().unwrap())
                .await
                .unwrap();

            let mut lock = call_handler.lock().await;

            let query_terms = [
                Some(sound_1),
                Some(sound_2),
                sound_3,
                sound_4,
                sound_5,
                sound_6,
                sound_7,
                sound_8,
                sound_9,
                sound_10,
                sound_11,
                sound_12,
                sound_13,
                sound_14,
                sound_15,
                sound_16,
                sound_17,
                sound_18,
                sound_19,
                sound_20,
                sound_21,
                sound_22,
                sound_23,
                sound_24,
                sound_25,
            ];

            let mut sounds = vec![];

            for sound in query_terms.iter().flatten() {
                let search = ctx
                    .data()
                    .search_for_sound(&sound, ctx.guild_id().unwrap(), ctx.author().id, true)
                    .await?;

                if let Some(sound) = search.first() {
                    sounds.push(sound.clone());
                }
            }

            queue_audio(
                &sounds,
                guild_data.read().await.volume,
                &mut lock,
                &ctx.data().database,
            )
            .await
            .unwrap();

            ctx.say(format!("Queued {} sounds!", sounds.len())).await?;
        }
        None => {
            ctx.say("You are not in a voice chat!").await?;
        }
    }

    Ok(())
}

/// Loop a sound in your current voice channel
#[poise::command(
    slash_command,
    rename = "loop",
    default_member_permissions = "SPEAK",
    guild_only = true
)]
pub async fn loop_play(
    ctx: Context<'_>,
    #[description = "Name or ID of sound to loop"]
    #[autocomplete = "autocomplete_sound"]
    name: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild = ctx.guild().unwrap();

    ctx.say(
        play_from_query(
            &ctx.discord(),
            &ctx.data(),
            guild,
            ctx.author().id,
            None,
            &name,
            true,
        )
        .await,
    )
    .await?;

    Ok(())
}

/// Get a menu of sounds with buttons to play them
#[poise::command(
    slash_command,
    rename = "soundboard",
    category = "Play",
    default_member_permissions = "SPEAK",
    guild_only = true
)]
pub async fn soundboard(
    ctx: Context<'_>,
    #[description = "Name or ID of sound for button 1"]
    #[autocomplete = "autocomplete_sound"]
    sound_1: String,
    #[description = "Name or ID of sound for button 2"]
    #[autocomplete = "autocomplete_sound"]
    sound_2: Option<String>,
    #[description = "Name or ID of sound for button 3"]
    #[autocomplete = "autocomplete_sound"]
    sound_3: Option<String>,
    #[description = "Name or ID of sound for button 4"]
    #[autocomplete = "autocomplete_sound"]
    sound_4: Option<String>,
    #[description = "Name or ID of sound for button 5"]
    #[autocomplete = "autocomplete_sound"]
    sound_5: Option<String>,
    #[description = "Name or ID of sound for button 6"]
    #[autocomplete = "autocomplete_sound"]
    sound_6: Option<String>,
    #[description = "Name or ID of sound for button 7"]
    #[autocomplete = "autocomplete_sound"]
    sound_7: Option<String>,
    #[description = "Name or ID of sound for button 8"]
    #[autocomplete = "autocomplete_sound"]
    sound_8: Option<String>,
    #[description = "Name or ID of sound for button 9"]
    #[autocomplete = "autocomplete_sound"]
    sound_9: Option<String>,
    #[description = "Name or ID of sound for button 10"]
    #[autocomplete = "autocomplete_sound"]
    sound_10: Option<String>,
    #[description = "Name or ID of sound for button 11"]
    #[autocomplete = "autocomplete_sound"]
    sound_11: Option<String>,
    #[description = "Name or ID of sound for button 12"]
    #[autocomplete = "autocomplete_sound"]
    sound_12: Option<String>,
    #[description = "Name or ID of sound for button 13"]
    #[autocomplete = "autocomplete_sound"]
    sound_13: Option<String>,
    #[description = "Name or ID of sound for button 14"]
    #[autocomplete = "autocomplete_sound"]
    sound_14: Option<String>,
    #[description = "Name or ID of sound for button 15"]
    #[autocomplete = "autocomplete_sound"]
    sound_15: Option<String>,
    #[description = "Name or ID of sound for button 16"]
    #[autocomplete = "autocomplete_sound"]
    sound_16: Option<String>,
    #[description = "Name or ID of sound for button 17"]
    #[autocomplete = "autocomplete_sound"]
    sound_17: Option<String>,
    #[description = "Name or ID of sound for button 18"]
    #[autocomplete = "autocomplete_sound"]
    sound_18: Option<String>,
    #[description = "Name or ID of sound for button 19"]
    #[autocomplete = "autocomplete_sound"]
    sound_19: Option<String>,
    #[description = "Name or ID of sound for button 20"]
    #[autocomplete = "autocomplete_sound"]
    sound_20: Option<String>,
    #[description = "Name or ID of sound for button 21"]
    #[autocomplete = "autocomplete_sound"]
    sound_21: Option<String>,
    #[description = "Name or ID of sound for button 22"]
    #[autocomplete = "autocomplete_sound"]
    sound_22: Option<String>,
    #[description = "Name or ID of sound for button 23"]
    #[autocomplete = "autocomplete_sound"]
    sound_23: Option<String>,
    #[description = "Name or ID of sound for button 24"]
    #[autocomplete = "autocomplete_sound"]
    sound_24: Option<String>,
    #[description = "Name or ID of sound for button 25"]
    #[autocomplete = "autocomplete_sound"]
    sound_25: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let query_terms = [
        Some(sound_1),
        sound_2,
        sound_3,
        sound_4,
        sound_5,
        sound_6,
        sound_7,
        sound_8,
        sound_9,
        sound_10,
        sound_11,
        sound_12,
        sound_13,
        sound_14,
        sound_15,
        sound_16,
        sound_17,
        sound_18,
        sound_19,
        sound_20,
        sound_21,
        sound_22,
        sound_23,
        sound_24,
        sound_25,
    ];

    let mut sounds = vec![];

    for sound in query_terms.iter().flatten() {
        let search = ctx
            .data()
            .search_for_sound(&sound, ctx.guild_id().unwrap(), ctx.author().id, true)
            .await?;

        if let Some(sound) = search.first() {
            if !sounds.contains(sound) {
                sounds.push(sound.clone());
            }
        }
    }

    ctx.send(|m| {
        m.content("**Play a sound:**").components(|c| {
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
        })
    })
    .await?;

    Ok(())
}
