use shared::{Command, ResponseCode, BUF_LEN, HEADER_LEN, MAX_MSG_LEN, RESPONSE_CODE_LEN};
use std::fmt;

enum QueryError {
    ReadFullError(shared::ReadFullError),
    WriteFullError(shared::WriteFullError),
    MessageTooLong(usize),
}

impl From<shared::ReadFullError> for QueryError {
    fn from(err: shared::ReadFullError) -> QueryError {
        QueryError::ReadFullError(err)
    }
}

impl From<shared::WriteFullError> for QueryError {
    fn from(err: shared::WriteFullError) -> QueryError {
        QueryError::WriteFullError(err)
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            QueryError::ReadFullError(err) => err.fmt(f),
            QueryError::WriteFullError(err) => err.fmt(f),
            QueryError::MessageTooLong(n) => write!(f, "message too long ({} bytes)", n),
        }
    }
}

fn write_arg_to_buf(arg: &[u8], buf: &mut Vec<u8>) {
    buf.extend_from_slice(&(arg.len() as u32).to_be_bytes());
    buf.extend_from_slice(arg);
}

fn execute_commands(fd: i32, commands: &[Command]) -> Result<(), QueryError> {
    // Write all commands

    let write_start = std::time::Instant::now();

    println!("writing all commands: {:?}", commands);

    let mut write_buf = Vec::with_capacity(BUF_LEN);

    write_buf.extend_from_slice(&(0 as u32).to_be_bytes()); // message length; placeholder for now

    for command in commands {
        match command {
            Command::Get(args) => {
                let total_args = (args.len() + 1) as u32;

                write_buf.extend_from_slice(&total_args.to_be_bytes()); // number of commands
                write_arg_to_buf(b"get", &mut write_buf);
                for arg in args {
                    write_arg_to_buf(arg, &mut write_buf);
                }
            }
            Command::Set(args) => {
                let total_args = (args.len() + 1) as u32;

                write_buf.extend_from_slice(&total_args.to_be_bytes()); // number of commands
                write_arg_to_buf(b"set", &mut write_buf);
                for arg in args {
                    write_arg_to_buf(arg, &mut write_buf);
                }
            }
            Command::Del(args) => {
                let total_args = (args.len() + 1) as u32;

                write_buf.extend_from_slice(&total_args.to_be_bytes()); // number of commands
                write_arg_to_buf(b"del", &mut write_buf);
                for arg in args {
                    write_arg_to_buf(arg, &mut write_buf);
                }
            }
        }
    }

    // Now we know the message length, write it
    let written = write_buf.len();
    write_buf[0..HEADER_LEN].copy_from_slice(&((written - HEADER_LEN) as u32).to_be_bytes());

    // TODO(vincent): do this before allocating
    if write_buf.len() > MAX_MSG_LEN {
        return Err(QueryError::MessageTooLong(write_buf.len()));
    }

    println!("client write buf: {:?}", &write_buf);

    shared::write_full(fd, &write_buf)?;

    let write_elapsed = std::time::Instant::now() - write_start;

    println!("wrote all queries in {:?}", write_elapsed);

    // Read all

    let read_start = std::time::Instant::now();

    println!("reading all resonses");

    for _ in 0..commands.len() {
        let mut read_buf: [u8; BUF_LEN] = [0; BUF_LEN];

        shared::read_full(fd, &mut read_buf[0..HEADER_LEN + RESPONSE_CODE_LEN])?;

        // Decode message length

        let message_len = {
            let data = &read_buf[0..HEADER_LEN];

            let len = u32::from_be_bytes(data.try_into().unwrap());
            len as usize
        };

        if message_len > MAX_MSG_LEN {
            return Err(QueryError::MessageTooLong(message_len));
        }

        // Decode response code

        let response_code: ResponseCode = {
            let data = &read_buf[HEADER_LEN..HEADER_LEN + RESPONSE_CODE_LEN];

            let tmp = u32::from_be_bytes(data.try_into().unwrap());

            tmp.try_into().unwrap()
        };

        println!("server says [{}]", response_code);
    }

    let read_elapsed = std::time::Instant::now() - read_start;

    println!("read all responses in {:?}", read_elapsed);

    Ok(())
}

fn main() -> Result<(), shared::MainError> {
    // Create socket

    let fd = shared::create_socket()?;

    println!("created socket fd={}", fd);

    // Connect

    let addr = shared::make_addr([127, 0, 0, 1], 1234);

    println!("connecting to 127.0.0.1:1234");

    shared::connect(fd, &addr)?;

    println!("connected to 127.0.0.1:1234");

    // Run multiple queries

    let command = Command::Set(vec![b"foo", b"bar"]);

    execute_commands(fd, &[command])?;

    println!("closing file descriptor fd={}", fd);

    shared::close(fd)?;

    Ok(())
}
