use std::env;

pub const THEME_COLOR: u32 = 0x00e0f3;

lazy_static! {
    pub static ref UPLOAD_MAX_SIZE: u64 = env::var("UPLOAD_MAX_SIZE")
        .unwrap_or_else(|_| "2097152".to_string())
        .parse::<u64>()
        .unwrap();
    pub static ref MAX_SOUNDS: u32 = env::var("MAX_SOUNDS")
        .unwrap_or_else(|_| "8".to_string())
        .parse::<u32>()
        .unwrap();
    pub static ref PATREON_GUILD: u64 = env::var("PATREON_GUILD").unwrap().parse::<u64>().unwrap();
    pub static ref PATREON_ROLE: u64 = env::var("PATREON_ROLE").unwrap().parse::<u64>().unwrap();
}
