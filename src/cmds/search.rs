use poise::{
    serenity_prelude,
    serenity_prelude::{
        application::component::ButtonStyle,
        constants::MESSAGE_CODE_LIMIT,
        interaction::{message_component::MessageComponentInteraction, InteractionResponseType},
        CreateActionRow, CreateEmbed, GuildId, UserId,
    },
    CreateReply,
};
use serde::{Deserialize, Serialize};

use crate::{
    consts::THEME_COLOR,
    models::sound::{Sound, SoundCtx},
    Context, Data, Error,
};

fn format_search_results<'a>(search_results: Vec<Sound>) -> CreateReply<'a> {
    let mut builder = CreateReply::default();

    let mut current_character_count = 0;
    let title = "Public sounds matching filter:";

    let field_iter = search_results
        .iter()
        .take(25)
        .map(|item| (&item.name, format!("ID: {}", item.id), true))
        .filter(|item| {
            current_character_count += item.0.len() + item.1.len();

            current_character_count <= MESSAGE_CODE_LIMIT - title.len()
        });

    builder.embed(|e| e.title(title).fields(field_iter));

    builder
}

/// Show uploaded sounds
#[poise::command(slash_command, rename = "list", guild_only = true)]
pub async fn list_sounds(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Copy)]
enum ListContext {
    User = 0,
    Guild = 1,
}

impl ListContext {
    pub fn title(&self) -> &'static str {
        match self {
            ListContext::User => "Your sounds",
            ListContext::Guild => "Server sounds",
        }
    }
}

/// Show the sounds uploaded to this server
#[poise::command(slash_command, rename = "server", guild_only = true)]
pub async fn list_guild_sounds(ctx: Context<'_>) -> Result<(), Error> {
    let pager = SoundPager {
        nonce: 0,
        page: 0,
        context: ListContext::Guild,
    };

    pager.reply(ctx).await?;

    Ok(())
}

/// Show all sounds you have uploaded
#[poise::command(slash_command, rename = "user", guild_only = true)]
pub async fn list_user_sounds(ctx: Context<'_>) -> Result<(), Error> {
    let pager = SoundPager {
        nonce: 0,
        page: 0,
        context: ListContext::User,
    };

    pager.reply(ctx).await?;

    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct SoundPager {
    nonce: u64,
    page: u64,
    context: ListContext,
}

impl SoundPager {
    async fn get_page(
        &self,
        data: &Data,
        user_id: UserId,
        guild_id: GuildId,
    ) -> Result<Vec<Sound>, sqlx::Error> {
        match self.context {
            ListContext::User => data.user_sounds(user_id, Some(self.page)).await,
            ListContext::Guild => data.guild_sounds(guild_id, Some(self.page)).await,
        }
    }

    fn create_action_row(&self, max_page: u64) -> CreateActionRow {
        let mut row = CreateActionRow::default();

        row.create_button(|b| {
            b.custom_id(
                serde_json::to_string(&SoundPager {
                    nonce: 0,
                    page: 0,
                    context: self.context,
                })
                .unwrap(),
            )
            .style(ButtonStyle::Primary)
            .label("⏪")
            .disabled(self.page == 0)
        })
        .create_button(|b| {
            b.custom_id(
                serde_json::to_string(&SoundPager {
                    nonce: 1,
                    page: self.page.saturating_sub(1),
                    context: self.context,
                })
                .unwrap(),
            )
            .style(ButtonStyle::Secondary)
            .label("◀️")
            .disabled(self.page == 0)
        })
        .create_button(|b| {
            b.custom_id("pid")
                .style(ButtonStyle::Success)
                .label(format!("Page {}", self.page + 1))
                .disabled(true)
        })
        .create_button(|b| {
            b.custom_id(
                serde_json::to_string(&SoundPager {
                    nonce: 2,
                    page: self.page.saturating_add(1),
                    context: self.context,
                })
                .unwrap(),
            )
            .style(ButtonStyle::Secondary)
            .label("▶️")
            .disabled(self.page == max_page)
        })
        .create_button(|b| {
            b.custom_id(
                serde_json::to_string(&SoundPager {
                    nonce: 3,
                    page: max_page,
                    context: self.context,
                })
                .unwrap(),
            )
            .style(ButtonStyle::Primary)
            .label("⏩")
            .disabled(self.page == max_page)
        });

        row
    }

    fn embed(&self, sounds: &[Sound], count: u64) -> CreateEmbed {
        let mut embed = CreateEmbed::default();

        embed
            .color(THEME_COLOR)
            .title(self.context.title())
            .description(format!("**{}** sounds:", count))
            .fields(sounds.iter().map(|s| {
                (
                    s.name.as_str(),
                    format!(
                        "ID: `{}`\n{}",
                        s.id,
                        if s.public { "*Public*" } else { "*Private*" }
                    ),
                    true,
                )
            }));

        embed
    }

    pub async fn handle_interaction(
        ctx: &serenity_prelude::Context,
        data: &Data,
        interaction: &MessageComponentInteraction,
    ) -> Result<(), Error> {
        let user_id = interaction.user.id;
        let guild_id = interaction.guild_id.unwrap();

        let pager = serde_json::from_str::<Self>(&interaction.data.custom_id)?;
        let sounds = pager.get_page(data, user_id, guild_id).await?;
        let count = match pager.context {
            ListContext::User => data.count_user_sounds(user_id).await?,
            ListContext::Guild => data.count_guild_sounds(guild_id).await?,
        };

        interaction
            .create_interaction_response(&ctx, |r| {
                r.kind(InteractionResponseType::UpdateMessage)
                    .interaction_response_data(|d| {
                        d.ephemeral(true)
                            .add_embed(pager.embed(&sounds, count))
                            .components(|c| c.add_action_row(pager.create_action_row(count / 25)))
                    })
            })
            .await?;

        Ok(())
    }

    async fn reply(&self, ctx: Context<'_>) -> Result<(), Error> {
        let sounds = self
            .get_page(ctx.data(), ctx.author().id, ctx.guild_id().unwrap())
            .await?;
        let count = match self.context {
            ListContext::User => ctx.data().count_user_sounds(ctx.author().id).await?,
            ListContext::Guild => {
                ctx.data()
                    .count_guild_sounds(ctx.guild_id().unwrap())
                    .await?
            }
        };

        ctx.send(|r| {
            r.ephemeral(true)
                .embed(|e| {
                    *e = self.embed(&sounds, count);
                    e
                })
                .components(|c| c.add_action_row(self.create_action_row(count / 25)))
        })
        .await?;

        Ok(())
    }
}

/// Search for sounds
#[poise::command(
    slash_command,
    rename = "search",
    category = "Search",
    guild_only = true
)]
pub async fn search_sounds(
    ctx: Context<'_>,
    #[description = "Sound name to search for"] query: String,
) -> Result<(), Error> {
    let search_results = ctx
        .data()
        .search_for_sound(&query, ctx.guild_id().unwrap(), ctx.author().id, false)
        .await?;

    ctx.send(|m| {
        *m = format_search_results(search_results);
        m
    })
    .await?;

    Ok(())
}

/// Show a page of random sounds
#[poise::command(slash_command, rename = "random", guild_only = true)]
pub async fn show_random_sounds(ctx: Context<'_>) -> Result<(), Error> {
    let search_results = sqlx::query_as_unchecked!(
        Sound,
        "
SELECT name, id, public, server_id, uploader_id
    FROM sounds
    WHERE public = 1
    ORDER BY rand()
    LIMIT 25
        "
    )
    .fetch_all(&ctx.data().database)
    .await?;

    ctx.send(|m| {
        *m = format_search_results(search_results);
        m
    })
    .await?;

    Ok(())
}
