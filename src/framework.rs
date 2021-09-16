use std::{
    collections::{HashMap, HashSet},
    env, fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

use log::{debug, error, info, warn};
use regex::{Match, Regex, RegexBuilder};
use serde_json::Value;
use serenity::{
    async_trait,
    builder::{CreateApplicationCommands, CreateComponents, CreateEmbed},
    cache::Cache,
    client::Context,
    framework::{standard::CommandResult, Framework},
    futures::prelude::future::BoxFuture,
    http::Http,
    model::{
        channel::{Channel, GuildChannel, Message},
        guild::{Guild, Member},
        id::{ChannelId, GuildId, RoleId, UserId},
        interactions::{
            application_command::{
                ApplicationCommand, ApplicationCommandInteraction, ApplicationCommandOptionType,
            },
            InteractionResponseType,
        },
    },
    prelude::TypeMapKey,
    Result as SerenityResult,
};

use crate::guild_data::CtxGuildData;

type CommandFn = for<'fut> fn(
    &'fut Context,
    &'fut (dyn CommandInvoke + Sync + Send),
    Args,
) -> BoxFuture<'fut, CommandResult>;

pub struct Args {
    pub args: HashMap<String, String>,
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

    pub fn named<D: ToString>(&self, name: D) -> Option<&String> {
        let name = name.to_string();

        self.args.get(&name)
    }
}

pub struct CreateGenericResponse {
    content: String,
    embed: Option<CreateEmbed>,
    components: Option<CreateComponents>,
}

impl CreateGenericResponse {
    pub fn new() -> Self {
        Self {
            content: "".to_string(),
            embed: None,
            components: None,
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

    pub fn components<F: FnOnce(&mut CreateComponents) -> &mut CreateComponents>(
        mut self,
        f: F,
    ) -> Self {
        let mut components = CreateComponents::default();
        f(&mut components);

        self.components = Some(components);
        self
    }
}

#[async_trait]
pub trait CommandInvoke {
    fn channel_id(&self) -> ChannelId;
    fn guild_id(&self) -> Option<GuildId>;
    fn guild(&self, cache: Arc<Cache>) -> Option<Guild>;
    fn author_id(&self) -> UserId;
    async fn member(&self, context: &Context) -> SerenityResult<Member>;
    fn msg(&self) -> Option<Message>;
    fn interaction(&self) -> Option<ApplicationCommandInteraction>;
    async fn respond(
        &self,
        http: Arc<Http>,
        generic_response: CreateGenericResponse,
    ) -> SerenityResult<()>;
    async fn followup(
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

    fn guild(&self, cache: Arc<Cache>) -> Option<Guild> {
        self.guild(cache)
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

    fn interaction(&self) -> Option<ApplicationCommandInteraction> {
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

                if let Some(components) = generic_response.components {
                    m.components(|c| {
                        *c = components;
                        c
                    });
                }

                m
            })
            .await
            .map(|_| ())
    }

    async fn followup(
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

                if let Some(components) = generic_response.components {
                    m.components(|c| {
                        *c = components;
                        c
                    });
                }

                m
            })
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl CommandInvoke for ApplicationCommandInteraction {
    fn channel_id(&self) -> ChannelId {
        self.channel_id
    }

    fn guild_id(&self) -> Option<GuildId> {
        self.guild_id
    }

    fn guild(&self, cache: Arc<Cache>) -> Option<Guild> {
        if let Some(guild_id) = self.guild_id {
            guild_id.to_guild_cached(cache)
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

    fn interaction(&self) -> Option<ApplicationCommandInteraction> {
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
                        d.add_embed(embed.clone());
                    }

                    if let Some(components) = generic_response.components {
                        d.components(|c| {
                            *c = components;
                            c
                        });
                    }

                    d
                })
        })
        .await
        .map(|_| ())
    }

    async fn followup(
        &self,
        http: Arc<Http>,
        generic_response: CreateGenericResponse,
    ) -> SerenityResult<()> {
        self.create_followup_message(http, |d| {
            d.content(generic_response.content);

            if let Some(embed) = generic_response.embed {
                d.add_embed(embed.clone());
            }

            if let Some(components) = generic_response.components {
                d.components(|c| {
                    *c = components;
                    c
                });
            }

            d
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

#[derive(Debug, PartialEq)]
pub enum CommandKind {
    Slash,
    Both,
    Text,
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
            ApplicationCommandOptionType::String => format!(r#"(?P<{}>.+?)"#, self.name),
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
    pub examples: &'static [&'static str],
    pub group: &'static str,

    pub kind: CommandKind,
    pub required_permissions: PermissionLevel,
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
                match ctx.guild_data(guild.id).await {
                    Ok(guild_data) => guild_data.read().await.allowed_role.map_or(true, |role| {
                        role == guild.id.0 || {
                            let role_id = RoleId(role);

                            member.roles.contains(&role_id)
                        }
                    }),

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
    pub commands: HashMap<String, &'static Command>,
    pub commands_: HashSet<&'static Command>,
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

        debug!("Command names: {}", command_names);

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

    fn _populate_commands<'a>(
        &self,
        commands: &'a mut CreateApplicationCommands,
    ) -> &'a mut CreateApplicationCommands {
        for command in &self.commands_ {
            commands.create_application_command(|c| {
                c.name(command.names[0]).description(command.desc);

                for arg in command.args {
                    c.create_option(|o| {
                        o.name(arg.name)
                            .description(arg.description)
                            .kind(arg.kind)
                            .required(arg.required)
                    });
                }

                c
            });
        }

        commands
    }

    pub async fn build_slash(&self, http: impl AsRef<Http>) {
        info!("Building slash commands...");

        match env::var("TEST_GUILD")
            .map(|i| i.parse::<u64>().ok())
            .ok()
            .flatten()
            .map(|i| GuildId(i))
        {
            None => {
                ApplicationCommand::set_global_application_commands(&http, |c| {
                    self._populate_commands(c)
                })
                .await
                .unwrap();
            }
            Some(debug_guild) => {
                debug_guild
                    .set_application_commands(&http, |c| self._populate_commands(c))
                    .await
                    .unwrap();
            }
        }

        info!("Slash commands built!");
    }

    pub async fn execute(&self, ctx: Context, interaction: ApplicationCommandInteraction) {
        let command = {
            self.commands.get(&interaction.data.name).expect(&format!(
                "Received invalid command: {}",
                interaction.data.name
            ))
        };

        if command
            .check_permissions(
                &ctx,
                &interaction.guild(ctx.cache.clone()).unwrap(),
                &interaction.clone().member.unwrap(),
            )
            .await
        {
            let mut args = HashMap::new();

            for arg in interaction
                .data
                .options
                .iter()
                .filter(|o| o.value.is_some())
            {
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
                        CreateGenericResponse::new().content("You must either be an Admin or have a role specified by `/roles` to do this command")
                    )
                    .await;
        } else if command.required_permissions == PermissionLevel::Restricted {
            let _ = interaction
                .respond(
                    ctx.http.clone(),
                    CreateGenericResponse::new().content("You must be an Admin to do this command"),
                )
                .await;
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
            let user_id = ctx.cache.current_user_id();

            let channel_perms = channel.permissions_for_user(ctx, user_id)?;

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
        else if let (Some(guild), Ok(Channel::Guild(channel))) =
            (msg.guild(&ctx), msg.channel(&ctx).await)
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

                                if command.kind != CommandKind::Slash {
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
                                    } else if command.required_permissions
                                        == PermissionLevel::Managed
                                    {
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
