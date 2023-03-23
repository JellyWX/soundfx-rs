# SoundFX

A bot for managing sound effects in Discord.

## Installing

Download a .deb file from the releases and install with `sudo apt install ./soundfx_rs-a.b.c_arm64.deb`. You will also need a database set up. Install MySQL 8.

## Running & config

The bot is installed as a systemd service `soundfx-rs`. Use `systemctl start soundfx-rs` and `systemctl stop soundfx-rs` to respectively start and stop the bot.

Config options are provided in a file `/etc/soundfx-rs/default.env`

Options:
* `DISCORD_TOKEN`- your token (required)
* `DATABASE_URL`- your database URL (required)
* `MAX_SOUNDS`- specifies how many sounds a user should be allowed without having the `PATREON_ROLE` specified below
* `PATREON_GUILD`- specifies the ID of the guild being used for Patreon benefits
* `PATREON_ROLE`- specifies the role being checked for Patreon benefits
* `CACHING_LOCATION`- specifies the location in which to cache the audio files (defaults to `/tmp/`)
* `UPLOAD_MAX_SIZE`- specifies the maximum upload size to permit in bytes. Defaults to 2MB

## Building from source

1. Install build dependencies: `sudo apt install gcc gcc-multilib cmake ffmpeg libopus-dev`
2. Install database server: `sudo apt install mysql-server-8.0`. Create a database called `soundfx`
3. Install Cargo and Rust from https://rustup.rs
4. Install SQLx CLI: `cargo install sqlx-cli`
5. From the source code directory, execute `sqlx migrate run`
6. Build with cargo: `cargo build --release`

When running from source, the config options above can be configured simply as environment variables.
