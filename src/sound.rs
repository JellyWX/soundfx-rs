use super::error::ErrorTypes;

use sqlx::mysql::MySqlPool;

use tokio::{fs::File, io::AsyncWriteExt, process::Command};

use songbird::input::restartable::Restartable;

use std::{env, path::Path};

pub struct Sound {
    pub name: String,
    pub id: u32,
    pub plays: u32,
    pub public: bool,
    pub server_id: u64,
    pub uploader_id: Option<u64>,
}

impl Sound {
    pub async fn search_for_sound(
        query: &str,
        guild_id: u64,
        user_id: u64,
        db_pool: MySqlPool,
        strict: bool,
    ) -> Result<Vec<Sound>, sqlx::Error> {
        fn extract_id(s: &str) -> Option<u32> {
            if s.len() > 3 && s.to_lowercase().starts_with("id:") {
                match s[3..].parse::<u32>() {
                    Ok(id) => Some(id),

                    Err(_) => None,
                }
            } else if let Ok(id) = s.parse::<u32>() {
                Some(id)
            } else {
                None
            }
        }

        if let Some(id) = extract_id(&query) {
            let sound = sqlx::query_as_unchecked!(
                Self,
                "
SELECT name, id, plays, public, server_id, uploader_id
    FROM sounds
    WHERE id = ? AND (
        public = 1 OR
        uploader_id = ? OR
        server_id = ?
    )
                ",
                id,
                user_id,
                guild_id
            )
            .fetch_all(&db_pool)
            .await?;

            Ok(sound)
        } else {
            let name = query;
            let sound;

            if strict {
                sound = sqlx::query_as_unchecked!(
                    Self,
                    "
SELECT name, id, plays, public, server_id, uploader_id
    FROM sounds
    WHERE name = ? AND (
        public = 1 OR
        uploader_id = ? OR
        server_id = ?
    )
    ORDER BY uploader_id = ? DESC, server_id = ? DESC, public = 1 DESC, rand()
                    ",
                    name,
                    user_id,
                    guild_id,
                    user_id,
                    guild_id
                )
                .fetch_all(&db_pool)
                .await?;
            } else {
                sound = sqlx::query_as_unchecked!(
                    Self,
                    "
SELECT name, id, plays, public, server_id, uploader_id
    FROM sounds
    WHERE name LIKE CONCAT('%', ?, '%') AND (
        public = 1 OR
        uploader_id = ? OR
        server_id = ?
    )
    ORDER BY uploader_id = ? DESC, server_id = ? DESC, public = 1 DESC, rand()
                    ",
                    name,
                    user_id,
                    guild_id,
                    user_id,
                    guild_id
                )
                .fetch_all(&db_pool)
                .await?;
            }

            Ok(sound)
        }
    }

    async fn src(&self, db_pool: MySqlPool) -> Vec<u8> {
        struct Src {
            src: Vec<u8>,
        }

        let record = sqlx::query_as_unchecked!(
            Src,
            "
SELECT src
    FROM sounds
    WHERE id = ?
    LIMIT 1
            ",
            self.id
        )
        .fetch_one(&db_pool)
        .await
        .unwrap();

        record.src
    }

    pub async fn store_sound_source(
        &self,
        db_pool: MySqlPool,
    ) -> Result<Restartable, Box<dyn std::error::Error + Send + Sync>> {
        let caching_location = env::var("CACHING_LOCATION").unwrap_or(String::from("/tmp"));

        let path_name = format!("{}/sound-{}", caching_location, self.id);
        let path = Path::new(&path_name);

        if !path.exists() {
            let mut file = File::create(&path).await?;

            file.write_all(&self.src(db_pool).await).await?;
        }

        Ok(Restartable::ffmpeg(path_name, false)
            .await
            .expect("FFMPEG ERROR!"))
    }

    pub async fn count_user_sounds(
        user_id: u64,
        db_pool: MySqlPool,
    ) -> Result<u32, sqlx::error::Error> {
        let c = sqlx::query!(
            "
SELECT COUNT(1) as count
    FROM sounds
    WHERE uploader_id = ?
        ",
            user_id
        )
        .fetch_one(&db_pool)
        .await?
        .count;

        Ok(c as u32)
    }

    pub async fn count_named_user_sounds(
        user_id: u64,
        name: &String,
        db_pool: MySqlPool,
    ) -> Result<u32, sqlx::error::Error> {
        let c = sqlx::query!(
            "
SELECT COUNT(1) as count
    FROM sounds
    WHERE
        uploader_id = ? AND
        name = ?
        ",
            user_id,
            name
        )
        .fetch_one(&db_pool)
        .await?
        .count;

        Ok(c as u32)
    }

    pub async fn set_as_greet(
        &self,
        user_id: u64,
        db_pool: MySqlPool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query!(
            "
UPDATE users
SET
    join_sound_id = ?
WHERE
    user = ?
            ",
            self.id,
            user_id
        )
        .execute(&db_pool)
        .await?;

        Ok(())
    }

    pub async fn commit(
        &self,
        db_pool: MySqlPool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query!(
            "
UPDATE sounds
SET
    plays = ?,
    public = ?
WHERE
    id = ?
            ",
            self.plays,
            self.public,
            self.id
        )
        .execute(&db_pool)
        .await?;

        Ok(())
    }

    pub async fn delete(
        &self,
        db_pool: MySqlPool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query!(
            "
DELETE
    FROM sounds
    WHERE id = ?
            ",
            self.id
        )
        .execute(&db_pool)
        .await?;

        Ok(())
    }

    pub async fn create_anon(
        name: &str,
        src_url: &str,
        server_id: u64,
        user_id: u64,
        db_pool: MySqlPool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + Send>> {
        async fn process_src(src_url: &str) -> Option<Vec<u8>> {
            let output = Command::new("ffmpeg")
                .kill_on_drop(true)
                .arg("-i")
                .arg(src_url)
                .arg("-loglevel")
                .arg("error")
                .arg("-b:a")
                .arg("28000")
                .arg("-f")
                .arg("opus")
                .arg("-fs")
                .arg("1048576")
                .arg("pipe:1")
                .output()
                .await;

            match output {
                Ok(out) => {
                    if out.status.success() {
                        Some(out.stdout)
                    } else {
                        None
                    }
                }

                Err(_) => None,
            }
        }

        let source = process_src(src_url).await;

        match source {
            Some(data) => {
                match sqlx::query!(
                    "
INSERT INTO sounds (name, server_id, uploader_id, public, src)
    VALUES (?, ?, ?, 1, ?)
                ",
                    name,
                    server_id,
                    user_id,
                    data
                )
                .execute(&db_pool)
                .await
                {
                    Ok(_) => Ok(()),

                    Err(e) => Err(Box::new(e)),
                }
            }

            None => Err(Box::new(ErrorTypes::InvalidFile)),
        }
    }

    pub async fn get_user_sounds(
        user_id: u64,
        db_pool: MySqlPool,
    ) -> Result<Vec<Sound>, Box<dyn std::error::Error + Send + Sync>> {
        let sounds = sqlx::query_as_unchecked!(
            Sound,
            "
SELECT name, id, plays, public, server_id, uploader_id
    FROM sounds
    WHERE uploader_id = ?
            ",
            user_id
        )
        .fetch_all(&db_pool)
        .await?;

        Ok(sounds)
    }

    pub async fn get_guild_sounds(
        guild_id: u64,
        db_pool: MySqlPool,
    ) -> Result<Vec<Sound>, Box<dyn std::error::Error + Send + Sync>> {
        let sounds = sqlx::query_as_unchecked!(
            Sound,
            "
SELECT name, id, plays, public, server_id, uploader_id
    FROM sounds
    WHERE server_id = ?
            ",
            guild_id
        )
        .fetch_all(&db_pool)
        .await?;

        Ok(sounds)
    }
}
