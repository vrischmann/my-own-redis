use onlyerror::Error;

use crate::protocol;

#[derive(Error, Debug)]
pub enum ParseCommandError {
    #[error("protocol error")]
    Protocol(#[from] protocol::Error),
    #[error("unknown command '{0}")]
    UnknownCommand(String),
}

pub type ParsedCommand<'a> = Vec<&'a [u8]>;

pub fn parse<'a>(body: &'a [u8]) -> Result<ParsedCommand<'a>, ParseCommandError> {
    let mut reader = protocol::Reader::new(body);

    // 1. Parse the number of arguments.

    let mut n_args = reader.read_u32()?;

    // 2. Parse each argument

    let mut args: Vec<&'a [u8]> = Vec::with_capacity(n_args as usize);
    while n_args > 0 {
        let arg = reader.read_string()?;

        n_args -= 1;

        args.push(arg);
    }

    if !KNOWN_COMMANDS.contains(&args[0]) {
        let cmd = String::from_utf8_lossy(args[0]).to_string();
        return Err(ParseCommandError::UnknownCommand(cmd));
    }

    Ok(args)
}

pub fn is_valid<T: AsRef<[u8]>>(value: T) -> bool {
    let cmd = value.as_ref();
    KNOWN_COMMANDS.contains(&cmd)
}

const KNOWN_COMMANDS: &'static [&'static [u8]] = &[b"get", b"set", b"del"];
