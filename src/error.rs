use std::fmt::Formatter;

#[derive(Debug)]
pub enum ErrorTypes {
    InvalidFile,
    NotEnoughRoles,
}

impl std::error::Error for ErrorTypes {}
impl std::fmt::Display for ErrorTypes {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ErrorTypes")
    }
}
