use serenity::model::guild::Guild;
use sqlx::mysql::MySqlPool;

pub struct GuildData {
    pub id: u64,
    pub name: Option<String>,
    pub prefix: String,
    pub volume: u8,
    pub allow_greets: bool,
}

impl GuildData {
    pub async fn get_from_id(guild: Guild, db_pool: MySqlPool) -> Option<GuildData> {
        let guild_data = sqlx::query_as_unchecked!(
            GuildData,
            "
SELECT id, name, prefix, volume, allow_greets
    FROM servers
    WHERE id = ?
            ", guild.id.as_u64()
        )
            .fetch_one(&db_pool)
            .await;

        match guild_data {
            Ok(g) => Some(g),

            Err(sqlx::Error::RowNotFound) => {
                Self::create_from_guild(guild, db_pool).await.ok()
            }

            Err(e) => {
                println!("{:?}", e);

                None
            }
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

        sqlx::query!(
            "
INSERT IGNORE INTO roles (guild_id, role)
    VALUES (?, ?)
            ",
            guild.id.as_u64(), guild.id.as_u64()
        )
            .execute(&db_pool)
            .await?;

        Ok(GuildData {
            id: *guild.id.as_u64(),
            name: Some(guild.name.clone()),
            prefix: String::from("?"),
            volume: 100,
            allow_greets: true
        })
    }

    pub async fn commit(&self, db_pool: MySqlPool) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query!(
            "
UPDATE servers
SET
    name = ?,
    prefix = ?,
    volume = ?,
    allow_greets = ?
WHERE
    id = ?
            ",
            self.name, self.prefix, self.volume, self.allow_greets, self.id
        )
            .execute(&db_pool)
            .await?;

        Ok(())
    }
}
