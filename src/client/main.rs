use onlyerror::Error;
use shared::protocol::{self, BUF_LEN, MAX_MSG_LEN};
use std::io;

#[derive(Error, Debug)]
enum QueryError {
    #[error("read_full error")]
    ReadFullError(#[from] shared::ReadFullError),
    #[error("i/o error")]
    IO(#[from] io::Error),
    #[error("protocol error")]
    Protocol(#[from] protocol::Error),
    #[error("message too long ({0} bytes)")]
    MessageTooLong(usize),
}

fn execute_commands(fd: i32, commands: &[Vec<&[u8]>]) -> Result<(), QueryError> {
    // Sanity checks

    let buffer_size_needed = protocol::buffer_size_needed(commands);
    if buffer_size_needed >= MAX_MSG_LEN {
        return Err(QueryError::MessageTooLong(buffer_size_needed));
    }

    // Write all commands

    let write_start = std::time::Instant::now();

    println!("writing all commands: {:?}", commands);

    let n_args: usize = commands.iter().map(|command| command.len()).sum();

    let write_buf = {
        let mut buf = Vec::with_capacity(BUF_LEN);
        buf.resize(BUF_LEN, 0xAA);

        let written = {
            let mut writer = shared::protocol::Writer::new(&mut buf);

            writer.push_int(n_args);

            for command in commands {
                let (cmd, args) = (command[0], &command[1..]);

                writer.push_string(cmd);
                for arg in args {
                    writer.push_string(arg);
                }
            }

            writer.finish();
            writer.written()
        };

        buf.truncate(written);

        buf
    };

    println!("client write buf: {:?}", &write_buf);

    shared::write_full(fd, &write_buf)?;

    let write_elapsed = std::time::Instant::now() - write_start;

    println!("wrote all queries in {:?}", write_elapsed);

    // Read all

    let read_start = std::time::Instant::now();

    println!("reading all responses");

    for _ in 0..commands.len() {
        let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];

        let read_buf = shared::read(fd, &mut buf)?;

        //

        // TODO(vincent): maybe better error handling ?
        let (_, message) = protocol::parse_message(read_buf).unwrap();

        let mut reader = protocol::Reader::new(message);

        match reader.read_data_type()? {
            protocol::DataType::Nil => {
                println!("nil");
            }
            protocol::DataType::Err => {
                let (response_code, message) = reader.read_err()?;

                println!("response code: {}", response_code);
                println!("message: {}", String::from_utf8_lossy(message));
            }
            protocol::DataType::Str => {
                let body = reader.read_string()?;

                println!(
                    "server says: {} (len={})",
                    String::from_utf8_lossy(body),
                    body.len(),
                );
            }
            protocol::DataType::Int => {
                todo!();
            }
            protocol::DataType::Arr => {
                todo!();
            }
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

    // if !command::is_valid(command[0]) {
    //     println!("Usage: my-own-redis <command> [<arg> ...]");
    //     std::process::exit(1);
    // }

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
