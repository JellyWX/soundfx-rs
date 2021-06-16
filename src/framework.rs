use serenity::{
    async_trait,
    builder::CreateEmbed,
    cache::Cache,
    client::Context,
    framework::{standard::CommandResult, Framework},
    futures::prelude::future::BoxFuture,
    http::Http,
    model::{
        channel::{Channel, GuildChannel, Message},
        guild::{Guild, Member},
        id::{ChannelId, GuildId, UserId},
        interactions::{ApplicationCommand, Interaction, InteractionType},
        prelude::{ApplicationCommandOptionType, InteractionResponseType},
    },
    prelude::TypeMapKey,
    Result as SerenityResult,
};

use log::{error, info, warn};

use regex::{Match, Regex, RegexBuilder};

use std::{
    collections::{HashMap, HashSet},
    env, fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

use crate::{guild_data::CtxGuildData, MySQL};
use serde_json::Value;

type CommandFn = for<'fut> fn(
    &'fut Context,
    &'fut (dyn CommandInvoke + Sync + Send),
    Args,
) -> BoxFuture<'fut, CommandResult>;

pub struct Args {
    args: HashMap<String, String>,
}

impl Args {
    pub fn from(message: &str, arg_schema: &'static [&'static Arg]) -> Self {
        // construct regex from arg schema
        let mut re = arg_schema
            .iter()
            .map(|a| a.to_regex())
            .collect::<Vec<String>>()
            .join(r#"\s*"#);

        re.push_str("$");

        let regex = Regex::new(&re).unwrap();
        let capture_names = regex.capture_names();
        let captures = regex.captures(message);

        let mut args = HashMap::new();

        if let Some(captures) = captures {
            for name in capture_names.filter(|n| n.is_some()).map(|n| n.unwrap()) {
                if let Some(cap) = captures.name(name) {
                    args.insert(name.to_string(), cap.as_str().to_string());
                }
            }
        }

        Self { args }
    }

    pub fn len(&self) -> usize {
        self.args.len()
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }

    pub fn named<D: ToString>(&self, name: D) -> Option<&String> {
        let name = name.to_string();

        self.args.get(&name)
    }
}

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
        if let Some(guild_id) = self.guild_id {
            guild_id.to_guild_cached(cache).await
        } else {
            None
        }
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

#[derive(Debug)]
pub struct Arg {
    pub name: &'static str,
    pub description: &'static str,
    pub kind: ApplicationCommandOptionType,
    pub required: bool,
}

impl Arg {
    pub fn to_regex(&self) -> String {
        match self.kind {
            ApplicationCommandOptionType::String => format!(r#"(?P<{}>.*?)"#, self.name),
            ApplicationCommandOptionType::Integer => format!(r#"(?P<{}>\d+)"#, self.name),
            ApplicationCommandOptionType::Boolean => format!(r#"(?P<{0}>{0})?"#, self.name),
            ApplicationCommandOptionType::User => format!(r#"<(@|@!)(?P<{}>\d+)>"#, self.name),
            ApplicationCommandOptionType::Channel => format!(r#"<#(?P<{}>\d+)>"#, self.name),
            ApplicationCommandOptionType::Role => format!(r#"<@&(?P<{}>\d+)>"#, self.name),
            ApplicationCommandOptionType::Mentionable => {
                format!(r#"<(?P<{0}_pref>@|@!|@&|#)(?P<{0}>\d+)>"#, self.name)
            }
            _ => String::new(),
        }
    }
}

pub struct Command {
    pub fun: CommandFn,
    pub names: &'static [&'static str],
    pub desc: &'static str,
    pub usage: Option<&'static str>,
    pub examples: &'static [&'static str],
    pub required_permissions: PermissionLevel,
    pub allow_slash: bool,
    pub args: &'static [&'static Arg],
}

impl Hash for Command {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.names[0].hash(state)
    }
}

impl PartialEq for Command {
    fn eq(&self, other: &Self) -> bool {
        self.names[0] == other.names[0]
    }
}

impl Eq for Command {}

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
            .field("args", &self.args)
            .finish()
    }
}

pub struct RegexFramework {
    commands: HashMap<String, &'static Command>,
    commands_: HashSet<&'static Command>,
    command_matcher: Regex,
    default_prefix: String,
    client_id: u64,
    ignore_bots: bool,
    case_insensitive: bool,
}

impl TypeMapKey for RegexFramework {
    type Value = Arc<RegexFramework>;
}

impl RegexFramework {
    pub fn new<T: Into<u64>>(client_id: T) -> Self {
        Self {
            commands: HashMap::new(),
            commands_: HashSet::new(),
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
        info!("{:?}", command);

        self.commands_.insert(command);

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

    pub async fn build_slash(&self, http: impl AsRef<Http>) {
        info!("Building slash commands...");

        let mut count = 0;

        if let Some(guild_id) = env::var("TEST_GUILD")
            .map(|v| v.parse::<u64>().ok())
            .ok()
            .flatten()
            .map(|v| GuildId(v))
        {
            for command in self.commands_.iter().filter(|c| c.allow_slash) {
                guild_id
                    .create_application_command(&http, |a| {
                        a.name(command.names[0]).description(command.desc);

                        for arg in command.args {
                            a.create_option(|o| {
                                o.name(arg.name)
                                    .description(arg.description)
                                    .kind(arg.kind)
                                    .required(arg.required)
                            });
                        }

                        a
                    })
                    .await
                    .expect(&format!(
                        "Failed to create application command for {}",
                        command.names[0]
                    ));

                count += 1;
            }
        } else {
            info!("Checking for existing commands...");

            let current_commands = ApplicationCommand::get_global_application_commands(&http)
                .await
                .expect("Failed to fetch existing commands");

            info!("Existing commands: {:?}", current_commands);

            // delete commands not in use
            for command in &current_commands {
                if self
                    .commands_
                    .iter()
                    .find(|c| c.names[0] == command.name)
                    .is_none()
                {
                    info!("Deleting command {}", command.name);

                    ApplicationCommand::delete_global_application_command(&http, command.id)
                        .await
                        .expect("Failed to delete an unused command");
                }
            }

            for command in self.commands_.iter().filter(|c| c.allow_slash) {
                let already_created = if let Some(current_command) = current_commands
                    .iter()
                    .find(|curr| curr.name == command.names[0])
                {
                    if current_command.description == command.desc
                        && current_command.options.len() == command.args.len()
                    {
                        let mut has_different_arg = false;

                        for (arg, option) in
                            command.args.iter().zip(current_command.options.clone())
                        {
                            if arg.required != option.required
                                || arg.name != option.name
                                || arg.description != option.description
                                || arg.kind != option.kind
                            {
                                has_different_arg = true;
                                break;
                            }
                        }

                        !has_different_arg
                    } else {
                        false
                    }
                } else {
                    false
                };

                if !already_created {
                    ApplicationCommand::create_global_application_command(&http, |a| {
                        a.name(command.names[0]).description(command.desc);

                        for arg in command.args {
                            a.create_option(|o| {
                                o.name(arg.name)
                                    .description(arg.description)
                                    .kind(arg.kind)
                                    .required(arg.required)
                            });
                        }

                        a
                    })
                    .await
                    .expect(&format!(
                        "Failed to create application command for {}",
                        command.names[0]
                    ));

                    count += 1;
                }
            }
        }

        info!("{} slash commands built! Ready to go", count);
    }

    pub async fn execute(&self, ctx: Context, interaction: Interaction) {
        if interaction.kind == InteractionType::ApplicationCommand && interaction.guild_id.is_some()
        {
            if let Some(data) = interaction.data.clone() {
                let command = {
                    let name = data.name;

                    self.commands
                        .get(&name)
                        .expect(&format!("Received invalid command: {}", name))
                };

                if command
                    .check_permissions(
                        &ctx,
                        &interaction.guild(ctx.cache.clone()).await.unwrap(),
                        &interaction.member(&ctx).await.unwrap(),
                    )
                    .await
                {
                    let mut args = HashMap::new();

                    for arg in data.options.iter().filter(|o| o.value.is_some()) {
                        args.insert(
                            arg.name.clone(),
                            match arg.value.clone().unwrap() {
                                Value::Bool(b) => {
                                    if b {
                                        arg.name.clone()
                                    } else {
                                        String::new()
                                    }
                                }
                                Value::Number(n) => n.to_string(),
                                Value::String(s) => s,
                                _ => String::new(),
                            },
                        );
                    }

                    (command.fun)(&ctx, &interaction, Args { args })
                        .await
                        .unwrap();
                } else if command.required_permissions == PermissionLevel::Managed {
                    let _ = interaction
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new().content("You must either be an Admin or have a role specified in `?roles` to do this command")
                        )
                        .await;
                } else if command.required_permissions == PermissionLevel::Restricted {
                    let _ = interaction
                        .respond(
                            ctx.http.clone(),
                            CreateGenericResponse::new()
                                .content("You must be an Admin to do this command"),
                        )
                        .await;
                }
            }
        }
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
                                    (command.fun)(&ctx, &msg, Args::from(&args, command.args))
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
