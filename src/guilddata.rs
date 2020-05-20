use serenity::model::guild::Guild;
use sqlx::mysql::MySqlPool;

pub struct GuildData {
    pub id: u64,
    pub name: Option<String>,
    pub prefix: String,
    pub volume: u8,
}

impl GuildData {
    pub async fn get_from_id(guild_id: u64, db_pool: MySqlPool) -> Option<GuildData> {
        let guild = sqlx::query_as!(
            GuildData,
            "
SELECT *
    FROM servers
    WHERE id = ?
            ", guild_id
        )
            .fetch_one(&db_pool)
            .await;

        match guild {
            Ok(guild) => Some(guild),

            Err(_) => None,
        }
    }

    pub async fn create_from_guild(guild: Guild, db_pool: MySqlPool) -> Result<GuildData, Box<dyn std::error::Error>> {
        sqlx::query!(
            "
INSERT INTO servers (id, name)
    VALUES (?, ?)
            ", guild.id.as_u64(), guild.name
        )
            .execute(&db_pool)
            .await?;

        Ok(GuildData {
            id: *guild.id.as_u64(),
            name: Some(guild.name.clone()),
            prefix: String::from("?"),
            volume: 100,
        })
    }

    pub async fn commit(&self, db_pool: MySqlPool) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query!(
            "
UPDATE servers
SET
    name = ?,
    prefix = ?,
    volume = ?
WHERE
    id = ?
            ",
            self.name, self.prefix, self.volume, self.id
        )
            .execute(&db_pool)
            .await?;

        Ok(())
    }
}
