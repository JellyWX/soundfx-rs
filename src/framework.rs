use serenity::{
    async_trait,
    client::Context,
    constants::MESSAGE_CODE_LIMIT,
    framework::{standard::Args, Framework},
    futures::prelude::future::BoxFuture,
    http::Http,
    model::{
        channel::{Channel, GuildChannel, Message},
        guild::{Guild, Member},
        id::ChannelId,
    },
    Result as SerenityResult,
};

use log::{error, info, warn};

use regex::{Match, Regex, RegexBuilder};

use std::{collections::HashMap, fmt};

use crate::{guild_data::GuildData, MySQL};
use serenity::framework::standard::{CommandResult, Delimiter};

type CommandFn = for<'fut> fn(&'fut Context, &'fut Message, Args) -> BoxFuture<'fut, CommandResult>;

#[derive(Debug, PartialEq)]
pub enum PermissionLevel {
    Unrestricted,
    Managed,
    Restricted,
}

pub struct Command {
    pub name: &'static str,
    pub required_perms: PermissionLevel,
    pub func: CommandFn,
}

impl Command {
    async fn check_permissions(&self, ctx: &Context, guild: &Guild, member: &Member) -> bool {
        if self.required_perms == PermissionLevel::Unrestricted {
            true
        } else {
            let permissions = guild.member_permissions(&ctx, &member.user).await.unwrap();

            if permissions.manage_guild() {
                return true;
            }

            if self.required_perms == PermissionLevel::Managed {
                let pool = ctx
                    .data
                    .read()
                    .await
                    .get::<MySQL>()
                    .cloned()
                    .expect("Could not get SQLPool from data");

                match sqlx::query!(
                    "
SELECT role
    FROM roles
    WHERE guild_id = ?
                    ",
                    guild.id.as_u64()
                )
                .fetch_all(&pool)
                .await
                {
                    Ok(rows) => {
                        let role_ids = member
                            .roles
                            .iter()
                            .map(|r| *r.as_u64())
                            .collect::<Vec<u64>>();

                        for row in rows {
                            if role_ids.contains(&row.role) || &row.role == guild.id.as_u64() {
                                return true;
                            }
                        }

                        false
                    }

                    Err(sqlx::Error::RowNotFound) => false,

                    Err(e) => {
                        warn!("Unexpected error occurred querying roles: {:?}", e);

                        false
                    }
                }
            } else {
                false
            }
        }
    }
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Command")
            .field("name", &self.name)
            .field("required_perms", &self.required_perms)
            .finish()
    }
}

#[async_trait]
pub trait SendIterator {
    async fn say_lines(
        self,
        http: impl AsRef<Http> + Send + Sync + 'async_trait,
        content: impl Iterator<Item = String> + Send + 'async_trait,
    ) -> SerenityResult<()>;
}

#[async_trait]
impl SendIterator for ChannelId {
    async fn say_lines(
        self,
        http: impl AsRef<Http> + Send + Sync + 'async_trait,
        content: impl Iterator<Item = String> + Send + 'async_trait,
    ) -> SerenityResult<()> {
        let mut current_content = String::new();

        for line in content {
            if current_content.len() + line.len() > MESSAGE_CODE_LIMIT as usize {
                self.send_message(&http, |m| {
                    m.allowed_mentions(|am| am.empty_parse())
                        .content(&current_content)
                })
                .await?;

                current_content = line;
            } else {
                current_content = format!("{}\n{}", current_content, line);
            }
        }
        if !current_content.is_empty() {
            self.send_message(&http, |m| {
                m.allowed_mentions(|am| am.empty_parse())
                    .content(&current_content)
            })
            .await?;
        }

        Ok(())
    }
}

pub struct RegexFramework {
    commands: HashMap<String, &'static Command>,
    command_matcher: Regex,
    default_prefix: String,
    client_id: u64,
    ignore_bots: bool,
    case_insensitive: bool,
}

impl RegexFramework {
    pub fn new<T: Into<u64>>(client_id: T) -> Self {
        Self {
            commands: HashMap::new(),
            command_matcher: Regex::new(r#"^$"#).unwrap(),
            default_prefix: "".to_string(),
            client_id: client_id.into(),
            ignore_bots: true,
            case_insensitive: true,
        }
    }

    pub fn case_insensitive(mut self, case_insensitive: bool) -> Self {
        self.case_insensitive = case_insensitive;

        self
    }

    pub fn default_prefix<T: ToString>(mut self, new_prefix: T) -> Self {
        self.default_prefix = new_prefix.to_string();

        self
    }

    pub fn ignore_bots(mut self, ignore_bots: bool) -> Self {
        self.ignore_bots = ignore_bots;

        self
    }

    pub fn add_command<S: ToString>(mut self, name: S, command: &'static Command) -> Self {
        self.commands.insert(name.to_string(), command);

        self
    }

    pub fn build(mut self) -> Self {
        let command_names;

        {
            let mut command_names_vec = self.commands.keys().map(|k| &k[..]).collect::<Vec<&str>>();

            command_names_vec.sort_unstable_by(|a, b| b.len().cmp(&a.len()));

            command_names = command_names_vec.join("|");
        }

        info!("Command names: {}", command_names);

        {
            let match_string = r#"^(?:(?:<@ID>\s*)|(?:<@!ID>\s*)|(?P<prefix>\S{1,5}?))(?P<cmd>COMMANDS)(?:$|\s+(?P<args>.*))$"#
                    .replace("COMMANDS", command_names.as_str())
                    .replace("ID", self.client_id.to_string().as_str());

            self.command_matcher = RegexBuilder::new(match_string.as_str())
                .case_insensitive(self.case_insensitive)
                .dot_matches_new_line(true)
                .build()
                .unwrap();
        }

        self
    }
}

enum PermissionCheck {
    None, // No permissions
    All,  // Sufficient permissions
}

#[async_trait]
impl Framework for RegexFramework {
    async fn dispatch(&self, ctx: Context, msg: Message) {
        async fn check_self_permissions(
            ctx: &Context,
            channel: &GuildChannel,
        ) -> SerenityResult<PermissionCheck> {
            let user_id = ctx.cache.current_user_id().await;

            let channel_perms = channel.permissions_for_user(ctx, user_id).await?;

            Ok(
                if channel_perms.send_messages() && channel_perms.embed_links() {
                    PermissionCheck::All
                } else {
                    PermissionCheck::None
                },
            )
        }

        async fn check_prefix(ctx: &Context, guild: &Guild, prefix_opt: Option<Match<'_>>) -> bool {
            if let Some(prefix) = prefix_opt {
                let pool = ctx
                    .data
                    .read()
                    .await
                    .get::<MySQL>()
                    .cloned()
                    .expect("Could not get SQLPool from data");

                let guild_prefix = match GuildData::get_from_id(guild.clone(), pool.clone()).await {
                    Some(guild_data) => guild_data.prefix,

                    None => {
                        GuildData::create_from_guild(guild, pool).await.unwrap();
                        String::from("?")
                    }
                };

                guild_prefix.as_str() == prefix.as_str()
            } else {
                true
            }
        }

        // gate to prevent analysing messages unnecessarily
        if msg.author.bot || msg.content.is_empty() {
        }
        // Guild Command
        else if let (Some(guild), Some(Channel::Guild(channel))) =
            (msg.guild(&ctx).await, msg.channel(&ctx).await)
        {
            if let Some(full_match) = self.command_matcher.captures(&msg.content) {
                if check_prefix(&ctx, &guild, full_match.name("prefix")).await {
                    match check_self_permissions(&ctx, &channel).await {
                        Ok(perms) => match perms {
                            PermissionCheck::All => {
                                let command = self
                                    .commands
                                    .get(&full_match.name("cmd").unwrap().as_str().to_lowercase())
                                    .unwrap();

                                let args = full_match
                                    .name("args")
                                    .map(|m| m.as_str())
                                    .unwrap_or("")
                                    .to_string();

                                let member = guild.member(&ctx, &msg.author).await.unwrap();

                                if command.check_permissions(&ctx, &guild, &member).await {
                                    dbg!(command.name);

                                    (command.func)(
                                        &ctx,
                                        &msg,
                                        Args::new(&args, &[Delimiter::Single(' ')]),
                                    )
                                    .await
                                    .unwrap();
                                } else if command.required_perms == PermissionLevel::Managed {
                                    let _ = msg.channel_id.say(&ctx, "You must either be an Admin or have a role specified in `?roles` to do this command").await;
                                } else if command.required_perms == PermissionLevel::Restricted {
                                    let _ = msg
                                        .channel_id
                                        .say(&ctx, "You must be an Admin to do this command")
                                        .await;
                                }
                            }

                            PermissionCheck::None => {
                                warn!("Missing enough permissions for guild {}", guild.id);
                            }
                        },

                        Err(e) => {
                            error!(
                                "Error occurred getting permissions in guild {}: {:?}",
                                guild.id, e
                            );
                        }
                    }
                }
            }
        }
    }
}
