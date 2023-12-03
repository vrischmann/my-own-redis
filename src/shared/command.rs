use std::borrow::Cow;

use onlyerror::Error;

use crate::STRING_LEN;

#[derive(Debug)]
pub enum Command<'a> {
    Get(Vec<&'a [u8]>),
    Set(Vec<&'a [u8]>),
    Del(Vec<&'a [u8]>),
}

#[derive(Error, Debug)]
pub enum ParseCommandError {
    #[error("input too short")]
    InputTooShort,
    #[error("unknown command '{0}")]
    UnknownCommand(String),
}

const ARGS_LEN: usize = 4;

impl<'a> Command<'a> {
    pub fn parse(body: &'a [u8]) -> Result<Self, ParseCommandError> {
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

        // We only care about the first argument for determining the command
        let (cmd, args) = (String::from_utf8_lossy(args[0]), &args[1..]);

        let command = match cmd {
            Cow::Borrowed("get") => Self::Get(args.to_vec()),
            Cow::Borrowed("set") => Self::Set(args.to_vec()),
            Cow::Borrowed("del") => Self::Del(args.to_vec()),
            cmd => return Err(ParseCommandError::UnknownCommand(cmd.to_string())),
        };

        Ok(command)
    }
}
