use crate::{consts::THEME_COLOR, Context, Error};

/// Get additional information about the bot
#[poise::command(slash_command, category = "Information")]
pub async fn info(ctx: Context<'_>) -> Result<(), Error> {
    let current_user = ctx.discord().cache.current_user();

    ctx.send(|m| m
        .embed(|e| e
            .title("Info")
            .color(THEME_COLOR)
            .footer(|f| f
                .text(concat!(env!("CARGO_PKG_NAME"), " ver ", env!("CARGO_PKG_VERSION"))))
            .description(format!("Default prefix: `?`

Reset prefix: `@{0} prefix ?`

Invite me: https://discord.com/api/oauth2/authorize?client_id={1}&permissions=3165184&scope=applications.commands%20bot

**Welcome to SoundFX!**
Developer: <@203532103185465344>
Find me on https://discord.jellywx.com/ and on https://github.com/JellyWX :)

**An online dashboard is available!** Visit https://soundfx.jellywx.com/dashboard
There is a maximum sound limit per user. This can be removed by subscribing at **https://patreon.com/jellywx**",
                                 current_user.name,
                                 current_user.id.as_u64())))).await?;

    Ok(())
}
