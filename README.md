# SoundFX

A bot for managing sound effects in Discord.

### Building

`sudo apt install gcc gcc-multilib cmake`

Run the migrations in the `migrations` directory to set up the database.

Use the Cargo.toml file to build it. Needs Rust 1.52+

### Running & Config

The bot connects to the MySQL server URL defined in a `.env` file in the working directory of the program.

Config options:
* `DISCORD_TOKEN`- your token (required)
* `DATABASE_URL`- your database URL (required)
* `MAX_SOUNDS`- specifies how many sounds a user should be allowed without Patreon
* `PATREON_GUILD`- specifies the ID of the guild being used for Patreon benefits
* `PATREON_ROLE`- specifies the role being checked for Patreon benefits
* `CACHING_LOCATION`- specifies the location in which to cache the audio files (defaults to `/tmp/`)
* `UPLOAD_MAX_SIZE`- specifies the maximum upload size to permit in bytes. Defaults to 2MB
