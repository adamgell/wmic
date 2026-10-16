mod command;
mod lex;
mod switches;
pub use command::{diagnostic_command_line, parse_command};

mod format;
pub use format::parse_format_spec;

/// True when the text lexes as exactly one token that spans all of it.
pub fn is_single_token(units: &[u16]) -> bool {
    lex::tokens(units).is_ok_and(|t| t.len() == 1 && t[0] == (0..units.len()))
}
