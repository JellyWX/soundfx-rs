use std::sync::Arc;

use poise::serenity::model::{
    channel::Channel,
    guild::Guild,
    id::{ChannelId, UserId},
};
use songbird::{create_player, error::JoinResult, tracks::TrackHandle, Call};
use sqlx::Executor;
use tokio::sync::{Mutex, MutexGuard};

use crate::{
    models::{
        guild_data::CtxGuildData,
        sound::{Sound, SoundCtx},
    },
    Data, Database,
};

pub async fn play_audio(
    sound: &mut Sound,
    volume: u8,
    call_handler: &mut MutexGuard<'_, Call>,
    db_pool: impl Executor<'_, Database = Database>,
    loop_: bool,
) -> Result<TrackHandle, Box<dyn std::error::Error + Send + Sync>> {
    let (track, track_handler) = create_player(sound.playable(db_pool).await?.into());

    let _ = track_handler.set_volume(volume as f32 / 100.0);

    if loop_ {
        let _ = track_handler.enable_loop();
    } else {
        let _ = track_handler.disable_loop();
    }

    call_handler.play(track);

    Ok(track_handler)
}

pub async fn join_channel(
    ctx: &poise::serenity_prelude::Context,
    guild: Guild,
    channel_id: ChannelId,
) -> (Arc<Mutex<Call>>, JoinResult<()>) {
    let songbird = songbird::get(ctx).await.unwrap();
    let current_user = ctx.cache.current_user_id();

    let current_voice_state = guild
        .voice_states
        .get(&current_user)
        .and_then(|voice_state| voice_state.channel_id);

    let (call, res) = if current_voice_state == Some(channel_id) {
        let call_opt = songbird.get(guild.id);

        if let Some(call) = call_opt {
            (call, Ok(()))
        } else {
            let (call, res) = songbird.join(guild.id, channel_id).await;

            (call, res)
        }
    } else {
        let (call, res) = songbird.join(guild.id, channel_id).await;

        (call, res)
    };

    {
        // set call to deafen
        let _ = call.lock().await.deafen(true).await;
    }

    if let Some(Channel::Guild(channel)) = channel_id.to_channel_cached(&ctx) {
        let _ = channel
            .edit_voice_state(&ctx, ctx.cache.current_user(), |v| v.suppress(false))
            .await;
    }

    (call, res)
}

pub async fn play_from_query(
    ctx: &poise::serenity_prelude::Context,
    data: &Data,
    guild: Guild,
    user_id: UserId,
    query: &str,
    loop_: bool,
) -> String {
    let guild_id = guild.id;

    let channel_to_join = guild
        .voice_states
        .get(&user_id)
        .and_then(|voice_state| voice_state.channel_id);

    match channel_to_join {
        Some(user_channel) => {
            let mut sound_vec = data
                .search_for_sound(query, guild_id, user_id, true)
                .await
                .unwrap();

            let sound_res = sound_vec.first_mut();

            match sound_res {
                Some(sound) => {
                    {
                        let (call_handler, _) =
                            join_channel(ctx, guild.clone(), user_channel).await;

                        let guild_data = data.guild_data(guild_id).await.unwrap();

                        let mut lock = call_handler.lock().await;

                        play_audio(
                            sound,
                            guild_data.read().await.volume,
                            &mut lock,
                            &data.database,
                            loop_,
                        )
                        .await
                        .unwrap();
                    }

                    format!("Playing sound {} with ID {}", sound.name, sound.id)
                }

                None => "Couldn't find sound by term provided".to_string(),
            }
        }

        None => "You are not in a voice chat!".to_string(),
    }
}
