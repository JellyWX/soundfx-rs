use crate::{models::sound::SoundCtx, Context};

pub mod info;
pub mod manage;
pub mod play;
pub mod search;
pub mod settings;
pub mod stop;

pub async fn autocomplete_sound(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<poise::AutocompleteChoice<String>> {
    ctx.data()
        .autocomplete_user_sounds(&partial, ctx.author().id, ctx.guild_id().unwrap())
        .await
        .unwrap_or(vec![])
        .iter()
        .map(|s| poise::AutocompleteChoice {
            name: s.name.clone(),
            value: s.id.to_string(),
        })
        .collect()
}
