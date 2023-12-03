use onlyerror::Error;
use shared::{
    command, ResponseCode, BUF_LEN, HEADER_LEN, MAX_MSG_LEN, RESPONSE_CODE_LEN, STRING_LEN,
};
use std::io;

#[derive(Error, Debug)]
enum QueryError {
    #[error("read_full error")]
    ReadFullError(#[from] shared::ReadFullError),
    #[error("i/o error")]
    IO(#[from] io::Error),
    #[error("message too long ({0} bytes)")]
    MessageTooLong(usize),
}

fn encode_string(arg: &[u8], buf: &mut Vec<u8>) {
    let len_bytes = &(arg.len() as u32).to_be_bytes();

    buf.extend_from_slice(len_bytes);
    buf.extend_from_slice(arg);
}

fn execute_commands(fd: i32, commands: &[Vec<&[u8]>]) -> Result<(), QueryError> {
    // Write all commands

    let write_start = std::time::Instant::now();

    println!("writing all commands: {:?}", commands);

    let mut write_buf = Vec::with_capacity(BUF_LEN);

    write_buf.extend_from_slice(&(0 as u32).to_be_bytes()); // message length; placeholder for now

    for command in commands {
        let (cmd, args) = (command[0], &command[1..]);

        write_buf.extend_from_slice(&(command.len() as u32).to_be_bytes()); // number of commands
        encode_string(cmd, &mut write_buf);
        for arg in args {
            encode_string(arg, &mut write_buf);
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

    println!("reading all responses");

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

        // Body

        let string_len = message_len - RESPONSE_CODE_LEN;
        if string_len > 0 {
            shared::read_full(fd, &mut read_buf[0..string_len])?;

            let data = &read_buf[0..string_len];

            let body_length = u32::from_be_bytes(data[0..STRING_LEN].try_into().unwrap());
            let body = &data[STRING_LEN..(STRING_LEN + body_length as usize)];

            println!(
                "server says [{}]: {} (len={})",
                response_code,
                String::from_utf8_lossy(body),
                body.len(),
            );
        } else {
            println!("server says [{}]", response_code);
        }
    }

    let read_elapsed = std::time::Instant::now() - read_start;

    println!("read all responses in {:?}", read_elapsed);

    Ok(())
}

fn main() -> anyhow::Result<()> {
    // Parse the command

    let mut args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: my-own-redis <command> [<arg> ...]");
        std::process::exit(1);
    }

    // Remove the binary name
    args.remove(0);
    // Construct the command and args
    let command: Vec<&[u8]> = args.iter().map(|v| v.as_ref()).collect();

    if !command::is_valid(command[0]) {
        println!("Usage: my-own-redis <command> [<arg> ...]");
        std::process::exit(1);
    }

    // Create socket

    let fd = shared::create_socket()?;

    println!("created socket fd={}", fd);

    // Connect

    let addr = shared::make_addr([127, 0, 0, 1], 1234);

    println!("connecting to 127.0.0.1:1234");

    shared::connect(fd, &addr)?;

    println!("connected to 127.0.0.1:1234");

    // Run multiple queries

    execute_commands(fd, &[command])?;

    println!("closing file descriptor fd={}", fd);

    shared::close(fd)?;

    Ok(())
}
