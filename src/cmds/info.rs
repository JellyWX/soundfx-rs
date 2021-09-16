use std::{collections::HashMap, sync::Arc};

use regex_command_attr::command;
use serenity::{client::Context, framework::standard::CommandResult};

use crate::{
    framework::{Args, CommandInvoke, CommandKind, CreateGenericResponse, RegexFramework},
    THEME_COLOR,
};

#[command]
#[group("Information")]
#[description("Get information on the commands of the bot")]
#[arg(
    name = "command",
    description = "Get help for a specific command",
    kind = "String",
    required = false
)]
#[example("`/help` - see all commands")]
#[example("`/help play` - get help about the `play` command")]
pub async fn help(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    args: Args,
) -> CommandResult {
    fn get_groups(framework: Arc<RegexFramework>) -> HashMap<&'static str, Vec<&'static str>> {
        let mut groups = HashMap::new();

        for command in &framework.commands_ {
            let entry = groups.entry(command.group).or_insert(vec![]);

            entry.push(command.names[0]);
        }

        groups
    }

    let framework = ctx
        .data
        .read()
        .await
        .get::<RegexFramework>()
        .cloned()
        .unwrap();

    if let Some(command_name) = args.named("command") {
        if let Some(command) = framework.commands.get(command_name) {
            let examples = if command.examples.is_empty() {
                "".to_string()
            } else {
                format!(
                    "**Examples**
{}",
                    command
                        .examples
                        .iter()
                        .map(|e| format!(" • {}", e))
                        .collect::<Vec<String>>()
                        .join("\n")
                )
            };

            let args = if command.args.is_empty() {
                "**Arguments**
 • *This command has no arguments*"
                    .to_string()
            } else {
                format!(
                    "**Arguments**
{}",
                    command
                        .args
                        .iter()
                        .map(|a| format!(
                            " • `{}` {} - {}",
                            a.name,
                            if a.required { "" } else { "[optional]" },
                            a.description
                        ))
                        .collect::<Vec<String>>()
                        .join("\n")
                )
            };

            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().embed(|e| {
                        e.title(format!("{} Help", command_name))
                            .color(THEME_COLOR)
                            .description(format!(
                                "**Available In**
`Slash Commands` {}
` Text Commands` {}

**Aliases**
{}

**Overview**
 • {}
{}

{}",
                                if command.kind != CommandKind::Text {
                                    "✅"
                                } else {
                                    "❎"
                                },
                                if command.kind != CommandKind::Slash {
                                    "✅"
                                } else {
                                    "❎"
                                },
                                command
                                    .names
                                    .iter()
                                    .map(|n| format!("`{}`", n))
                                    .collect::<Vec<String>>()
                                    .join(" "),
                                command.desc,
                                args,
                                examples
                            ))
                    }),
                )
                .await?;
        } else {
            let groups = get_groups(framework);
            let groups_iter = groups.iter().map(|(name, commands)| {
                (
                    name,
                    commands
                        .iter()
                        .map(|c| format!("`{}`", c))
                        .collect::<Vec<String>>()
                        .join(" "),
                    true,
                )
            });

            invoke
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().embed(|e| {
                        e.title("Invalid Command")
                            .color(THEME_COLOR)
                            .description("Type `/help command` to view more about a command below:")
                            .fields(groups_iter)
                    }),
                )
                .await?;
        }
    } else {
        let groups = get_groups(framework);
        let groups_iter = groups.iter().map(|(name, commands)| {
            (
                name,
                commands
                    .iter()
                    .map(|c| format!("`{}`", c))
                    .collect::<Vec<String>>()
                    .join(" "),
                true,
            )
        });

        invoke
            .respond(
                ctx.http.clone(),
                CreateGenericResponse::new().embed(|e| {
                    e.title("Help")
                        .color(THEME_COLOR)
                        .description("**Welcome to SoundFX!**
To get started, upload a sound with `/upload`, or use `/search` and `/play` to look at some of the public sounds

Type `/help command` to view help about a command below:")
                        .fields(groups_iter)
                }),
            )
            .await?;
    }

    Ok(())
}

#[command]
#[group("Information")]
#[aliases("invite")]
#[description("Get additional information on the bot")]
async fn info(
    ctx: &Context,
    invoke: &(dyn CommandInvoke + Sync + Send),
    _args: Args,
) -> CommandResult {
    let current_user = ctx.cache.current_user();

    invoke.respond(ctx.http.clone(), CreateGenericResponse::new()
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

**Sound Credits**
\"The rain falls against the parasol\" https://freesound.org/people/straget/
\"Heavy Rain\" https://freesound.org/people/lebaston100/
\"Rain on Windows, Interior, A\" https://freesound.org/people/InspectorJ/
\"Seaside Waves, Close, A\" https://freesound.org/people/InspectorJ/
\"Small River 1 - Fast - Close\" https://freesound.org/people/Pfannkuchn/

**An online dashboard is available!** Visit https://soundfx.jellywx.com/dashboard
There is a maximum sound limit per user. This can be removed by subscribing at **https://patreon.com/jellywx**", current_user.name, current_user.id.as_u64())))).await?;

    Ok(())
}
