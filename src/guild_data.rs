use crate::{GuildDataCache, MySQL};
use serenity::{async_trait, model::id::GuildId, prelude::Context};
use sqlx::mysql::MySqlPool;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct GuildData {
    pub id: u64,
    pub prefix: String,
    pub volume: u8,
    pub allow_greets: bool,
}

#[async_trait]
pub trait CtxGuildData {
    async fn get_from_id<G: Into<GuildId> + Send + Sync>(
        &self,
        guild_id: G,
    ) -> Result<Arc<RwLock<GuildData>>, sqlx::Error>;
}

#[async_trait]
impl CtxGuildData for Context {
    async fn get_from_id<G: Into<GuildId> + Send + Sync>(
        &self,
        guild_id: G,
    ) -> Result<Arc<RwLock<GuildData>>, sqlx::Error> {
        let guild_id = guild_id.into();

        let guild_cache = self
            .data
            .read()
            .await
            .get::<GuildDataCache>()
            .cloned()
            .unwrap();

        let x = if let Some(guild_data) = guild_cache.get(&guild_id) {
            Ok(guild_data.clone())
        } else {
            let pool = self.data.read().await.get::<MySQL>().cloned().unwrap();

            match GuildData::get_from_id(guild_id, pool).await {
                Ok(d) => {
                    let lock = Arc::new(RwLock::new(d));

                    guild_cache.insert(guild_id, lock.clone());

                    Ok(lock)
                }

                Err(e) => Err(e),
            }
        };

        x
    }
}

impl GuildData {
    pub async fn get_from_id<G: Into<GuildId>>(
        guild_id: G,
        db_pool: MySqlPool,
    ) -> Result<GuildData, sqlx::Error> {
        let guild_id = guild_id.into();

        let guild_data = sqlx::query_as_unchecked!(
            GuildData,
            "
SELECT id, prefix, volume, allow_greets
    FROM servers
    WHERE id = ?
            ",
            guild_id.as_u64()
        )
        .fetch_one(&db_pool)
        .await;

        match guild_data {
            Err(sqlx::error::Error::RowNotFound) => {
                Self::create_from_guild(guild_id, db_pool).await
            }

            d => d,
        }
    }

    async fn create_from_guild<G: Into<GuildId>>(
        guild_id: G,
        db_pool: MySqlPool,
    ) -> Result<GuildData, sqlx::Error> {
        let guild_id = guild_id.into();

        sqlx::query!(
            "
INSERT INTO servers (id)
    VALUES (?)
            ",
            guild_id.as_u64()
        )
        .execute(&db_pool)
        .await?;

        sqlx::query!(
            "
INSERT IGNORE INTO roles (guild_id, role)
    VALUES (?, ?)
            ",
            guild_id.as_u64(),
            guild_id.as_u64()
        )
        .execute(&db_pool)
        .await?;

        Ok(GuildData {
            id: guild_id.as_u64().to_owned(),
            prefix: String::from("?"),
            volume: 100,
            allow_greets: true,
        })
    }

    pub async fn commit(
        &self,
        db_pool: MySqlPool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query!(
            "
UPDATE servers
SET
    prefix = ?,
    volume = ?,
    allow_greets = ?
WHERE
    id = ?
            ",
            self.prefix,
            self.volume,
            self.allow_greets,
            self.id
        )
        .execute(&db_pool)
        .await?;

        Ok(())
    }
}
