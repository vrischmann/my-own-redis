use onlyerror::Error;

use crate::protocol;

#[derive(Error, Debug)]
pub enum ParseCommandError {
    #[error("protocol error")]
    Protocol(#[from] protocol::Error),
}

pub type ParsedCommand<'a> = Vec<&'a [u8]>;

pub fn parse<'a>(body: &'a [u8]) -> Result<ParsedCommand<'a>, ParseCommandError> {
    println!("==> body: {:?}", body);

    let mut reader = protocol::Reader::new(body);

    // 1. Parse the number of arguments.

    let mut n_args = {
        let _ = reader.read_data_type_expecting(protocol::DataType::Int)?;
        reader.read_int()?
    };

    // 2. Parse each argument

    let mut args: Vec<&'a [u8]> = Vec::with_capacity(n_args as usize);
    while n_args > 0 {
        let arg = {
            let _ = reader.read_data_type_expecting(protocol::DataType::Str)?;
            reader.read_string()?
        };

        n_args -= 1;

        args.push(arg);
    }

    Ok(args)
}

pub fn is_valid<T: AsRef<[u8]>>(value: T) -> bool {
    let cmd = value.as_ref();
    KNOWN_COMMANDS.contains(&cmd)
}

const KNOWN_COMMANDS: &[&[u8]] = &[b"get", b"set", b"del"];
