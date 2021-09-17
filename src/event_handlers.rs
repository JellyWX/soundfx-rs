use std::{collections::HashMap, env};

use serenity::{
    async_trait,
    client::{Context, EventHandler},
    model::{
        channel::Channel,
        gateway::{Activity, Ready},
        guild::Guild,
        id::GuildId,
        interactions::{Interaction, InteractionResponseType},
        voice::VoiceState,
    },
    utils::shard_id,
};
use songbird::{Event, EventContext, EventHandler as SongbirdEventHandler};

use crate::{
    framework::{Args, RegexFramework},
    guild_data::CtxGuildData,
    join_channel, play_audio, play_from_query,
    sound::{JoinSoundCtx, Sound},
    MySQL, ReqwestClient,
};

pub struct RestartTrack;

#[async_trait]
impl SongbirdEventHandler for RestartTrack {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(&[(_state, track)]) = ctx {
            let _ = track.seek_time(Default::default());
        }

        None
    }
}

pub struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _: Ready) {
        ctx.set_activity(Activity::watching("for /play")).await;
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: bool) {
        if is_new {
            if let Ok(token) = env::var("DISCORDBOTS_TOKEN") {
                let shard_count = ctx.cache.shard_count();
                let current_shard_id = shard_id(guild.id.as_u64().to_owned(), shard_count);

                let guild_count = ctx
                    .cache
                    .guilds()
                    .iter()
                    .filter(|g| shard_id(g.as_u64().to_owned(), shard_count) == current_shard_id)
                    .count() as u64;

                let mut hm = HashMap::new();
                hm.insert("server_count", guild_count);
                hm.insert("shard_id", current_shard_id);
                hm.insert("shard_count", shard_count);

                let client = ctx
                    .data
                    .read()
                    .await
                    .get::<ReqwestClient>()
                    .cloned()
                    .expect("Could not get ReqwestClient from data");

                let response = client
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

    async fn voice_state_update(
        &self,
        ctx: Context,
        guild_id_opt: Option<GuildId>,
        old: Option<VoiceState>,
        new: VoiceState,
    ) {
        if let Some(past_state) = old {
            if let (Some(guild_id), None) = (guild_id_opt, new.channel_id) {
                if let Some(channel_id) = past_state.channel_id {
                    if let Some(Channel::Guild(channel)) = channel_id.to_channel_cached(&ctx) {
                        if channel.members(&ctx).await.map(|m| m.len()).unwrap_or(0) <= 1 {
                            let songbird = songbird::get(&ctx).await.unwrap();

                            let _ = songbird.remove(guild_id).await;
                        }
                    }
                }
            }
        } else if let (Some(guild_id), Some(user_channel)) = (guild_id_opt, new.channel_id) {
            if let Some(guild) = ctx.cache.guild(guild_id) {
                let pool = ctx
                    .data
                    .read()
                    .await
                    .get::<MySQL>()
                    .cloned()
                    .expect("Could not get SQLPool from data");

                let guild_data_opt = ctx.guild_data(guild.id).await;

                if let Ok(guild_data) = guild_data_opt {
                    let volume;
                    let allowed_greets;

                    {
                        let read = guild_data.read().await;

                        volume = read.volume;
                        allowed_greets = read.allow_greets;
                    }

                    if allowed_greets {
                        if let Some(join_id) = ctx.join_sound(new.user_id).await {
                            let mut sound = sqlx::query_as_unchecked!(
                                Sound,
                                "
SELECT name, id, plays, public, server_id, uploader_id
    FROM sounds
    WHERE id = ?
                                        ",
                                join_id
                            )
                            .fetch_one(&pool)
                            .await
                            .unwrap();

                            let (handler, _) = join_channel(&ctx, guild, user_channel).await;

                            let _ = play_audio(
                                &mut sound,
                                volume,
                                &mut handler.lock().await,
                                pool,
                                false,
                            )
                            .await;
                        }
                    }
                }
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::ApplicationCommand(application_command) => {
                if application_command.guild_id.is_none() {
                    return;
                }

                let framework = ctx
                    .data
                    .read()
                    .await
                    .get::<RegexFramework>()
                    .cloned()
                    .expect("RegexFramework not found in context");

                framework.execute(ctx, application_command).await;
            }
            Interaction::MessageComponent(component) => {
                if component.guild_id.is_none() {
                    return;
                }

                let mut args = Args {
                    args: Default::default(),
                };
                args.args
                    .insert("query".to_string(), component.data.custom_id.clone());

                play_from_query(
                    &ctx,
                    component.guild_id.unwrap().to_guild_cached(&ctx).unwrap(),
                    component.user.id,
                    args,
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
            _ => {}
        }
    }
}
