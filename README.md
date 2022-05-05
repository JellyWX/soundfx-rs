# SoundFX

A bot for managing sound effects in Discord.

### Building

`sudo apt install gcc gcc-multilib cmake`

Use the Cargo.toml file to build it. Needs Rust 1.52+

### Running & Config

The bot connects to the MySQL server URL defined in a `.env` file in the working directory of the program.

Config options:
* `DISCORD_TOKEN`- your token (required)
* `DATABASE_URL`- your database URL (required)
* `DISCONNECT_CYCLES`- specifies the number of inactivity cycles before the bot should disconnect itself from a voice channel
* `DISCONNECT_CYCLE_DELAY`- specifies the delay between cleanup cycles
* `MAX_SOUNDS`- specifies how many sounds a user should be allowed without Patreon
* `PATREON_GUILD`- specifies the ID of the guild being used for Patreon benefits
* `PATREON_ROLE`- specifies the role being checked for Patreon benefits
* `CACHING_LOCATION`- specifies the location in which to cache the audio files (defaults to `/tmp/`)
