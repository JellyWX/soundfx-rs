use poise::serenity::{async_trait, model::id::UserId};

use crate::Data;

#[async_trait]
pub trait JoinSoundCtx {
    async fn join_sound<U: Into<UserId> + Send + Sync>(&self, user_id: U) -> Option<u32>;
    async fn update_join_sound<U: Into<UserId> + Send + Sync>(
        &self,
        user_id: U,
        join_id: Option<u32>,
    );
}

#[async_trait]
impl JoinSoundCtx for Data {
    async fn join_sound<U: Into<UserId> + Send + Sync>(&self, user_id: U) -> Option<u32> {
        let user_id = user_id.into();

        let x = if let Some(join_sound_id) = self.join_sound_cache.get(&user_id) {
            join_sound_id.value().clone()
        } else {
            let join_sound_id = {
                let pool = self.database.clone();

                let join_id_res = sqlx::query!(
                    "
SELECT join_sound_id
    FROM users
    WHERE user = ?
                    ",
                    user_id.as_u64()
                )
                .fetch_one(&pool)
                .await;

                if let Ok(row) = join_id_res {
                    row.join_sound_id
                } else {
                    None
                }
            };

            self.join_sound_cache.insert(user_id, join_sound_id);

            join_sound_id
        };

        x
    }

    async fn update_join_sound<U: Into<UserId> + Send + Sync>(
        &self,
        user_id: U,
        join_id: Option<u32>,
    ) {
        let user_id = user_id.into();

        self.join_sound_cache.insert(user_id, join_id);

        let pool = self.database.clone();

        let _ = sqlx::query!(
            "
INSERT IGNORE INTO users (user)
    VALUES (?)
            ",
            user_id.as_u64()
        )
        .execute(&pool)
        .await;

        let _ = sqlx::query!(
            "
UPDATE users
SET
    join_sound_id = ?
WHERE
    user = ?
            ",
            join_id,
            user_id.as_u64()
        )
        .execute(&pool)
        .await;
    }
}
