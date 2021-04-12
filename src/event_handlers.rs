use serenity::async_trait;
use songbird::Event;
use songbird::EventContext;
use songbird::EventHandler as SongbirdEventHandler;

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
