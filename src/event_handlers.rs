use crate::{
    framework::RegexFramework,
    guild_data::CtxGuildData,
    join_channel, play_audio,
    sound::{JoinSoundCtx, Sound},
    MySQL, ReqwestClient,
};

use serenity::{
    async_trait,
    client::{Context, EventHandler},
    model::{
        channel::Channel, guild::Guild, id::GuildId, interactions::Interaction, voice::VoiceState,
    },
    utils::shard_id,
};

use songbird::{Event, EventContext, EventHandler as SongbirdEventHandler};

use std::{collections::HashMap, env};

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
    async fn cache_ready(&self, ctx: Context, _: Vec<GuildId>) {
        let framework = ctx
            .data
            .read()
            .await
            .get::<RegexFramework>()
            .cloned()
            .expect("RegexFramework not found in context");

        framework.build_slash(ctx).await;
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: bool) {
        if is_new {
            if let Ok(token) = env::var("DISCORDBOTS_TOKEN") {
                let shard_count = ctx.cache.shard_count().await;
                let current_shard_id = shard_id(guild.id.as_u64().to_owned(), shard_count);

                let guild_count = ctx
                    .cache
                    .guilds()
                    .await
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
                            ctx.cache.current_user_id().await.as_u64()
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
                    if let Some(Channel::Guild(channel)) = channel_id.to_channel_cached(&ctx).await
                    {
                        if channel.members(&ctx).await.map(|m| m.len()).unwrap_or(0) <= 1 {
                            let songbird = songbird::get(&ctx).await.unwrap();

                            let _ = songbird.remove(guild_id).await;
                        }
                    }
                }
            }
        } else if let (Some(guild_id), Some(user_channel)) = (guild_id_opt, new.channel_id) {
            if let Some(guild) = ctx.cache.guild(guild_id).await {
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
        //
    }
}
