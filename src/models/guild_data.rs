use std::sync::Arc;

use poise::serenity::{async_trait, model::id::GuildId};
use sqlx::Executor;
use tokio::sync::RwLock;

use crate::{Context, Data, Database};

#[derive(Clone)]
pub struct GuildData {
    pub id: u64,
    pub prefix: String,
    pub volume: u8,
    pub allow_greets: bool,
    pub allowed_role: Option<u64>,
}

#[async_trait]
pub trait CtxGuildData {
    async fn guild_data<G: Into<GuildId> + Send + Sync>(
        &self,
        guild_id: G,
    ) -> Result<Arc<RwLock<GuildData>>, sqlx::Error>;
}

#[async_trait]
impl CtxGuildData for Context<'_> {
    async fn guild_data<G: Into<GuildId> + Send + Sync>(
        &self,
        guild_id: G,
    ) -> Result<Arc<RwLock<GuildData>>, sqlx::Error> {
        self.data().guild_data(guild_id).await
    }
}

#[async_trait]
impl CtxGuildData for Data {
    async fn guild_data<G: Into<GuildId> + Send + Sync>(
        &self,
        guild_id: G,
    ) -> Result<Arc<RwLock<GuildData>>, sqlx::Error> {
        let guild_id = guild_id.into();

        let x = if let Some(guild_data) = self.guild_data_cache.get(&guild_id) {
            Ok(guild_data.clone())
        } else {
            match GuildData::from_id(guild_id, &self.database).await {
                Ok(d) => {
                    let lock = Arc::new(RwLock::new(d));

                    self.guild_data_cache.insert(guild_id, lock.clone());

                    Ok(lock)
                }

                Err(e) => Err(e),
            }
        };

        x
    }
}

impl GuildData {
    pub async fn from_id<G: Into<GuildId>>(
        guild_id: G,
        db_pool: impl Executor<'_, Database = Database> + Copy,
    ) -> Result<GuildData, sqlx::Error> {
        let guild_id = guild_id.into();

        let guild_data = sqlx::query_as_unchecked!(
            GuildData,
            "
SELECT id, prefix, volume, allow_greets, allowed_role
    FROM servers
    WHERE id = ?
            ",
            guild_id.as_u64()
        )
        .fetch_one(db_pool)
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
        db_pool: impl Executor<'_, Database = Database>,
    ) -> Result<GuildData, sqlx::Error> {
        let guild_id = guild_id.into();

        sqlx::query!(
            "
INSERT INTO servers (id)
    VALUES (?)
            ",
            guild_id.as_u64()
        )
        .execute(db_pool)
        .await?;

        Ok(GuildData {
            id: guild_id.as_u64().to_owned(),
            prefix: String::from("?"),
            volume: 100,
            allow_greets: true,
            allowed_role: None,
        })
    }

    pub async fn commit(
        &self,
        db_pool: impl Executor<'_, Database = Database>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query!(
            "
UPDATE servers
SET
    prefix = ?,
    volume = ?,
    allow_greets = ?,
    allowed_role = ?
WHERE
    id = ?
            ",
            self.prefix,
            self.volume,
            self.allow_greets,
            self.allowed_role,
            self.id
        )
        .execute(db_pool)
        .await?;

        Ok(())
    }
}
