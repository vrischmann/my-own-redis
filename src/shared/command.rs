use onlyerror::Error;

use crate::STRING_LEN;

#[derive(Error, Debug)]
pub enum ParseCommandError {
    #[error("input too short")]
    InputTooShort,
    #[error("unknown command '{0}")]
    UnknownCommand(String),
}

const ARGS_LEN: usize = 4;

pub type ParsedCommand<'a> = Vec<&'a [u8]>;

pub fn parse<'a>(body: &'a [u8]) -> Result<ParsedCommand<'a>, ParseCommandError> {
    let mut body = body;

    if body.len() < ARGS_LEN {
        return Err(ParseCommandError::InputTooShort);
    }

    // 1. Parse the number of arguments.

    let mut n_args = u32::from_be_bytes(body[0..ARGS_LEN].try_into().unwrap());
    // "consume" the bytes we just used
    body = &body[ARGS_LEN..];

    // 2. Parse each argument

    let mut args: Vec<&'a [u8]> = Vec::with_capacity(n_args as usize);
    while n_args > 0 {
        if body.len() <= 0 {
            return Err(ParseCommandError::InputTooShort);
        }

        // An argument is a length-prefixed string:
        // * 4 bytes of length
        // * N bytes of string data

        let string_length = u32::from_be_bytes(body[0..STRING_LEN].try_into().unwrap());

        let arg = &body[STRING_LEN..STRING_LEN + string_length as usize];
        args.push(arg);

        n_args -= 1;

        // "consume" the bytes we just used
        body = &body[STRING_LEN + string_length as usize..];
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
