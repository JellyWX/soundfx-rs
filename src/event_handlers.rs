use std::{collections::HashMap, env};

use poise::{
    serenity::{
        model::{
            channel::Channel,
            interactions::{Interaction, InteractionResponseType},
        },
        prelude::Context,
        utils::shard_id,
    },
    serenity_prelude::Activity,
};

use crate::{
    models::{guild_data::CtxGuildData, join_sound::JoinSoundCtx, sound::Sound},
    utils::{join_channel, play_audio, play_from_query},
    Data, Error,
};

pub async fn listener(ctx: &Context, event: &poise::Event<'_>, data: &Data) -> Result<(), Error> {
    match event {
        poise::Event::CacheReady { .. } => {
            ctx.set_activity(Activity::watching("for /play")).await;
        }
        poise::Event::GuildCreate { guild, is_new, .. } => {
            if *is_new {
                if let Ok(token) = env::var("DISCORDBOTS_TOKEN") {
                    let shard_count = ctx.cache.shard_count();
                    let current_shard_id = shard_id(guild.id.as_u64().to_owned(), shard_count);

                    let guild_count = ctx
                        .cache
                        .guilds()
                        .iter()
                        .filter(|g| {
                            shard_id(g.as_u64().to_owned(), shard_count) == current_shard_id
                        })
                        .count() as u64;

                    let mut hm = HashMap::new();
                    hm.insert("server_count", guild_count);
                    hm.insert("shard_id", current_shard_id);
                    hm.insert("shard_count", shard_count);

                    let response = data
                        .http
                        .post(
                            format!(
                                "https://top.gg/api/bots/{}/stats",
                                ctx.cache.current_user_id().as_u64()
                            )
                            .as_str(),
                        )
                        .header("Authorization", token)
                        .json(&hm)
                        .send()
                        .await;

                    if let Err(res) = response {
                        println!("DiscordBots Response: {:?}", res);
                    }
                }
            }
        }
        poise::Event::VoiceStateUpdate { old, new, .. } => {
            if let Some(past_state) = old {
                if let (Some(guild_id), None) = (past_state.guild_id, new.channel_id) {
                    if let Some(channel_id) = past_state.channel_id {
                        if let Some(Channel::Guild(channel)) = channel_id.to_channel_cached(&ctx) {
                            if channel.members(&ctx).await.map(|m| m.len()).unwrap_or(0) <= 1 {
                                let songbird = songbird::get(ctx).await.unwrap();

                                let _ = songbird.remove(guild_id).await;
                            }
                        }
                    }
                }
            } else if let (Some(guild_id), Some(user_channel)) = (new.guild_id, new.channel_id) {
                if let Some(guild) = ctx.cache.guild(guild_id) {
                    let guild_data_opt = data.guild_data(guild.id).await;

                    if let Ok(guild_data) = guild_data_opt {
                        let volume;
                        let allowed_greets;

                        {
                            let read = guild_data.read().await;

                            volume = read.volume;
                            allowed_greets = read.allow_greets;
                        }

                        if allowed_greets {
                            if let Some(join_id) = data.join_sound(new.user_id).await {
                                let mut sound = sqlx::query_as_unchecked!(
                                    Sound,
                                    "
SELECT name, id, public, server_id, uploader_id
    FROM sounds
    WHERE id = ?
                                        ",
                                    join_id
                                )
                                .fetch_one(&data.database)
                                .await
                                .unwrap();

                                let (handler, _) = join_channel(&ctx, guild, user_channel).await;

                                play_audio(
                                    &mut sound,
                                    volume,
                                    &mut handler.lock().await,
                                    &data.database,
                                    false,
                                )
                                .await
                                .unwrap();
                            }
                        }
                    }
                }
            }
        }
        poise::Event::InteractionCreate { interaction } => match interaction {
            Interaction::MessageComponent(component) => {
                if component.guild_id.is_some() {
                    play_from_query(
                        &ctx,
                        &data,
                        component.guild_id.unwrap().to_guild_cached(&ctx).unwrap(),
                        component.user.id,
                        &component.data.custom_id,
                        false,
                    )
                    .await;

                    component
                        .create_interaction_response(ctx, |r| {
                            r.kind(InteractionResponseType::DeferredUpdateMessage)
                        })
                        .await
                        .unwrap();
                }
            }
            _ => {}
        },
        _ => {}
    }

    Ok(())
}
