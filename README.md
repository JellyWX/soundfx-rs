# SoundFX 2
## The complete (second) Rust rewrite of SoundFX

SoundFX 2 is the Rust rewrite of SoundFX. SoundFX 2 attempts to retain all functionality of the original bot, in a more 
efficient and robust package. SoundFX 2 is as asynchronous as it can get, and runs on the Tokio runtime.

### Building

Run the migrations in the `migrations` directory to set up the database.

Use Cargo to build the executable.

### Running & Config

The bot connects to the MySQL server URL defined in the environment.

Environment variables read:
* `DISCORD_TOKEN`- your token (required)
* `DATABASE_URL`- your database URL (required)
* `UPLOAD_MAX_SIZE`- specifies the maximum file size to allow in bytes (defaults to 2097152 (2MB))
* `MAX_SOUNDS`- specifies how many sounds a user should be allowed without Patreon
* `PATREON_GUILD`- specifies the ID of the guild being used for Patreon benefits
* `PATREON_ROLE`- specifies the role being checked for Patreon benefits
* `CACHING_LOCATION`- specifies the location in which to cache the audio files (defaults to `/tmp/`)

The bot will also consider variables in a `.env` file in the working directory.
