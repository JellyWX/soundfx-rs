use serenity::async_trait;
use serenity::model::id::GuildId;
use songbird::Event;
use songbird::EventContext;
use songbird::EventHandler as SongbirdEventHandler;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

pub struct UpdateTrackCount {
    pub guild_id: GuildId,
    pub track_count: Arc<RwLock<HashMap<GuildId, u32>>>,
}

#[async_trait]
impl SongbirdEventHandler for UpdateTrackCount {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        {
            let mut write_lock = self.track_count.write().await;

            let current = write_lock.get(&self.guild_id).cloned();
            write_lock.insert(self.guild_id, current.unwrap_or(1) - 1);
        }

        None
    }
}
