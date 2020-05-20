# SoundFX 2
## The complete (second) Rust rewrite of SoundFX

SoundFX 2 is the Rust rewrite of SoundFX. SoundFX 2 attempts to retain all functionality of the original bot, in a more 
efficient and robust package. SoundFX 2 is as asynchronous as it can get, and runs on the Tokio runtime.

### Building

Use the Cargo.toml file to build it. Simple as. Don't need any shit like MySQL libs and stuff because SQLx includes its 
own pure Rust one. Needs Rust 1.43+

### Running & Config

The bot connects to the MySQL server URL defined in a `.env` file in the working directory of the program.

Config options:
* `DISCORD_TOKEN`- your token (required)
* `DATABASE_URL`- your database URL (required)
* `DISCONNECT_CYCLES`- specifies the number of inactivity cycles before the bot should disconnect itself from a voice channel
* `DISCONNECT_CYCLE_LENGTH`- specifies the delay between cleanup cycles
* `MAX_SOUNDS`- specifies how many sounds a user should be allowed without Patreon
* `PATREON_GUILD`- specifies the ID of the guild being used for Patreon benefits
* `PATREON_ROLE`- specifies the role being checked for Patreon benefits
* `CACHING_LOCATION`- specifies the location in which to cache the audio files (defaults to `/tmp/`)

