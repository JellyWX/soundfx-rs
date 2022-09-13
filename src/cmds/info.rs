use crate::{consts::THEME_COLOR, Context, Error};

/// View bot commands
#[poise::command(slash_command)]
pub async fn help(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(|m| {
        m.embed(|e| {
            e.title("Help")
                .color(THEME_COLOR)
                .footer(|f| {
                    f.text(concat!(
                        env!("CARGO_PKG_NAME"),
                        " ver ",
                        env!("CARGO_PKG_VERSION")
                    ))
                })
                .description(
                    "__Info Commands__
`/help` `/info`
*run these commands with no options*

__Play Commands__
`/play` - Play a sound by name or ID
`/queue` - Play sounds on queue instead of instantly
`/loop` - Play a sound on loop
`/disconnect` - Disconnect the bot
`/stop` - Stop playback

__Library Commands__
`/upload` - Upload a sound file
`/delete` - Delete a sound file
`/download` - Download a sound file
`/public` - Set a sound as public/private
`/list server` - List sounds on this server
`/list user` - List your sounds

__Search Commands__
`/search` - Search for public sounds by name
`/random` - View random public sounds

__Setting Commands__
`/greet server set/unset` - Set or unset a join sound for just this server
`/greet user set/unset` - Set or unset a join sound across all servers
`/greet enable/disable` - Enable or disable join sounds on this server
`/volume` - Change the volume

__Advanced Commands__
`/soundboard` - Create a soundboard",
                )
        })
    })
    .await?;

    Ok(())
}

/// Get additional information about the bot
#[poise::command(slash_command)]
pub async fn info(ctx: Context<'_>) -> Result<(), Error> {
    let current_user = ctx.discord().cache.current_user();

    ctx.send(|m| m
        .embed(|e| e
            .title("Info")
            .color(THEME_COLOR)
            .footer(|f| f
                .text(concat!(env!("CARGO_PKG_NAME"), " ver ", env!("CARGO_PKG_VERSION"))))
            .description(format!("Invite me: https://discord.com/api/oauth2/authorize?client_id={}&permissions=3165184&scope=applications.commands%20bot

**Welcome to SoundFX!**
Developer: <@203532103185465344>
Find me on https://discord.jellywx.com/ and on https://github.com/JellyWX :)

**An online dashboard is available!** Visit https://soundfx.jellywx.com/dashboard
There is a maximum sound limit per user. This can be removed by subscribing at **https://patreon.com/jellywx**",
                                 current_user.id.as_u64())))).await?;

    Ok(())
}
