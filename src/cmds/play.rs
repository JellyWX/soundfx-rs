use poise::serenity::{
    builder::CreateActionRow, model::interactions::message_component::ButtonStyle,
};

use crate::{
    cmds::autocomplete_sound, models::sound::SoundCtx, utils::play_from_query, Context, Error,
};

/// Play a sound in your current voice channel
#[poise::command(slash_command)]
pub async fn play(
    ctx: Context<'_>,
    #[description = "Name or ID of sound to play"]
    #[autocomplete = "autocomplete_sound"]
    name: String,
) -> Result<(), Error> {
    let guild = ctx.guild().unwrap();

    ctx.say(
        play_from_query(
            &ctx.discord(),
            &ctx.data(),
            guild,
            ctx.author().id,
            &name,
            false,
        )
        .await,
    )
    .await?;

    Ok(())
}

/// Loop a sound in your current voice channel
#[poise::command(slash_command, rename = "loop")]
pub async fn loop_play(
    ctx: Context<'_>,
    #[description = "Name or ID of sound to loop"]
    #[autocomplete = "autocomplete_sound"]
    name: String,
) -> Result<(), Error> {
    let guild = ctx.guild().unwrap();

    ctx.say(
        play_from_query(
            &ctx.discord(),
            &ctx.data(),
            guild,
            ctx.author().id,
            &name,
            true,
        )
        .await,
    )
    .await?;

    Ok(())
}

/// Get a menu of sounds with buttons to play them
#[poise::command(slash_command, rename = "soundboard", category = "Play")]
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
