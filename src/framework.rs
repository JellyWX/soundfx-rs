use serenity::{
    async_trait,
    client::Context,
    constants::MESSAGE_CODE_LIMIT,
    framework::{
        standard::{Args, CommandResult, Delimiter},
        Framework,
    },
    futures::prelude::future::BoxFuture,
    http::Http,
    model::{
        channel::{Channel, GuildChannel, Message},
        guild::{Guild, Member},
        id::{ChannelId, GuildId, UserId},
        interactions::Interaction,
    },
    Result as SerenityResult,
};

use log::{error, info, warn};

use regex::{Match, Regex, RegexBuilder};

use std::{collections::HashMap, fmt};

use crate::{guild_data::CtxGuildData, MySQL};
use serenity::builder::CreateEmbed;
use serenity::cache::Cache;
use serenity::model::prelude::InteractionResponseType;
use std::sync::Arc;

type CommandFn = for<'fut> fn(
    &'fut Context,
    &'fut (dyn CommandInvoke + Sync + Send),
    Args,
) -> BoxFuture<'fut, CommandResult>;

pub struct CreateGenericResponse {
    content: String,
    embed: Option<CreateEmbed>,
}

impl CreateGenericResponse {
    pub fn new() -> Self {
        Self {
            content: "".to_string(),
            embed: None,
        }
    }

    pub fn content<D: ToString>(mut self, content: D) -> Self {
        self.content = content.to_string();

        self
    }

    pub fn embed<F: FnOnce(&mut CreateEmbed) -> &mut CreateEmbed>(mut self, f: F) -> Self {
        let mut embed = CreateEmbed::default();

        f(&mut embed);

        self.embed = Some(embed);

        self
    }
}

#[async_trait]
pub trait CommandInvoke {
    fn channel_id(&self) -> ChannelId;
    fn guild_id(&self) -> Option<GuildId>;
    async fn guild(&self, cache: Arc<Cache>) -> Option<Guild>;
    fn author_id(&self) -> UserId;
    async fn member(&self, context: &Context) -> SerenityResult<Member>;
    fn msg(&self) -> Option<Message>;
    fn interaction(&self) -> Option<Interaction>;
    async fn respond(
        &self,
        http: Arc<Http>,
        generic_response: CreateGenericResponse,
    ) -> SerenityResult<()>;
}

#[async_trait]
impl CommandInvoke for Message {
    fn channel_id(&self) -> ChannelId {
        self.channel_id
    }

    fn guild_id(&self) -> Option<GuildId> {
        self.guild_id
    }

    async fn guild(&self, cache: Arc<Cache>) -> Option<Guild> {
        self.guild(cache).await
    }

    fn author_id(&self) -> UserId {
        self.author.id
    }

    async fn member(&self, context: &Context) -> SerenityResult<Member> {
        self.member(context).await
    }

    fn msg(&self) -> Option<Message> {
        Some(self.clone())
    }

    fn interaction(&self) -> Option<Interaction> {
        None
    }

    async fn respond(
        &self,
        http: Arc<Http>,
        generic_response: CreateGenericResponse,
    ) -> SerenityResult<()> {
        self.channel_id
            .send_message(http, |m| {
                m.content(generic_response.content);

                if let Some(embed) = generic_response.embed {
                    m.set_embed(embed.clone());
                }

                m
            })
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl CommandInvoke for Interaction {
    fn channel_id(&self) -> ChannelId {
        self.channel_id.unwrap()
    }

    fn guild_id(&self) -> Option<GuildId> {
        self.guild_id
    }

    async fn guild(&self, cache: Arc<Cache>) -> Option<Guild> {
        self.guild(cache).await
    }

    fn author_id(&self) -> UserId {
        self.member.as_ref().unwrap().user.id
    }

    async fn member(&self, _: &Context) -> SerenityResult<Member> {
        Ok(self.member.clone().unwrap())
    }

    fn msg(&self) -> Option<Message> {
        None
    }

    fn interaction(&self) -> Option<Interaction> {
        Some(self.clone())
    }

    async fn respond(
        &self,
        http: Arc<Http>,
        generic_response: CreateGenericResponse,
    ) -> SerenityResult<()> {
        self.create_interaction_response(http, |r| {
            r.kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|d| {
                    d.content(generic_response.content);

                    if let Some(embed) = generic_response.embed {
                        d.set_embed(embed.clone());
                    }

                    d
                })
        })
        .await
        .map(|_| ())
    }
}

#[derive(Debug, PartialEq)]
pub enum PermissionLevel {
    Unrestricted,
    Managed,
    Restricted,
}

pub struct Command {
    pub fun: CommandFn,
    pub names: &'static [&'static str],
    pub desc: Option<&'static str>,
    pub usage: Option<&'static str>,
    pub examples: &'static [&'static str],
    pub required_permissions: PermissionLevel,
    pub allow_slash: bool,
}

impl Command {
    async fn check_permissions(&self, ctx: &Context, guild: &Guild, member: &Member) -> bool {
        if self.required_permissions == PermissionLevel::Unrestricted {
            true
        } else {
            let permissions = guild.member_permissions(&ctx, &member.user).await.unwrap();

            if permissions.manage_guild() {
                return true;
            }

            if self.required_permissions == PermissionLevel::Managed {
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
            .field("name", &self.names[0])
            .field("required_permissions", &self.required_permissions)
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

    pub fn add_command(mut self, command: &'static Command) -> Self {
        for name in command.names {
            self.commands.insert(name.to_string(), command);
        }

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
                match ctx.guild_data(guild.id).await {
                    Ok(guild_data) => prefix.as_str() == guild_data.read().await.prefix,

                    Err(_) => prefix.as_str() == "?",
                }
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
                                    (command.fun)(
                                        &ctx,
                                        &msg,
                                        Args::new(&args, &[Delimiter::Single(' ')]),
                                    )
                                    .await
                                    .unwrap();
                                } else if command.required_permissions == PermissionLevel::Managed {
                                    let _ = msg.channel_id.say(&ctx, "You must either be an Admin or have a role specified in `?roles` to do this command").await;
                                } else if command.required_permissions
                                    == PermissionLevel::Restricted
                                {
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
