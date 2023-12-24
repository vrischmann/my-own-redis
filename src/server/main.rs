use connection_buffer::ConnectionBuffer;
use error_iter::ErrorIter as _;
use hash_map::SuperHashMap;
use libc::{POLLERR, POLLIN, POLLOUT};
use libc::{SOMAXCONN, SO_REUSEADDR};
use onlyerror::Error;
use shared::ResponseCode;
use shared::{command, protocol};
use std::collections::HashMap;
use std::io;
use std::mem;

mod connection_buffer;
mod hash_map;

struct Context {
    data: SuperHashMap<String, String>,
}

#[derive(Debug)]
enum State {
    ReadRequest,
    SendResponse,
}

struct Connection {
    fd: i32,
    state: State,

    read_buf: ConnectionBuffer,
    write_buf: ConnectionBuffer,
}

#[derive(Error, Debug)]
enum TryFillBufferError {
    #[error("try_one_request failed")]
    TryOneRequest(#[from] TryOneRequestError),
    #[error("i/o error")]
    IO(#[from] io::Error),
    #[error("end of stream")]
    EndOfStream,
}

fn try_fill_buffer(
    context: &mut Context,
    connection: &mut Connection,
) -> Result<bool, TryFillBufferError> {
    // Remove the already processed requests from the buffer, if any
    connection.read_buf.remove_processed();

    //

    let read = loop {
        let buf = connection.read_buf.writable();
        match shared::read(connection.fd, buf) {
            Ok(data) => {
                if data.is_empty() {
                    return Err(TryFillBufferError::EndOfStream);
                } else {
                    break data.len();
                }
            }
            Err(err) => {
                if err.raw_os_error().unwrap() != libc::EAGAIN {
                    return Err(TryFillBufferError::IO(err));
                }
                return Ok(false);
            }
        }
    };

    connection.read_buf.update_write_head(read);

    // Try to process requests
    loop {
        if !try_one_request(context, connection)? {
            break;
        }
    }

    // Try to send the responses

    connection.state = State::SendResponse;
    do_send_responses(connection);

    if let State::ReadRequest = connection.state {
        Ok(true)
    } else {
        Ok(false)
    }
}

#[derive(Error, Debug)]
enum TryOneRequestError {
    #[error("do_request failed")]
    DoRequest(#[from] DoRequestError),
    #[error("protocol error")]
    Protocol(#[from] protocol::Error),
}

fn try_one_request(
    context: &mut Context,
    connection: &mut Connection,
) -> Result<bool, TryOneRequestError> {
    // Parse the request

    let (parsed, message) = match protocol::parse_message(connection.read_buf.readable()) {
        Ok(request) => request,
        Err(err) => match err {
            protocol::Error::MessageTooLong(_)
            | protocol::Error::InvalidDataType(_)
            | protocol::Error::InvalidResponseCode(_)
            | protocol::Error::IncoherentDataType { .. } => return Err(err.into()),
            protocol::Error::InputTooShort(_) => return Ok(false),
        },
    };

    println!(
        "request body: {:?} ({})",
        message,
        String::from_utf8_lossy(message)
    );

    // Process the request
    {
        let written = do_request(context, message, connection.write_buf.writable())?;

        connection.write_buf.update_write_head(written);

        println!(
            "write buf in try_one_request: {:?}",
            connection.write_buf.readable()
        );
    }

    // "consume" the bytes of the current request
    connection.read_buf.update_read_head(parsed);

    // Continue the outer loop if the request was fully processed
    match connection.state {
        State::ReadRequest => Ok(true),
        _ => Ok(false),
    }
}

#[derive(Error, Debug)]
enum DoRequestError {
    #[error("command parsing failed")]
    ParseCommand(#[from] shared::command::ParseCommandError),
}

fn do_request(
    context: &mut Context,
    body: &[u8],
    write_buf: &mut [u8],
) -> Result<usize, DoRequestError> {
    println!("client says {:?}", body);

    let mut writer = protocol::Writer::new(write_buf);

    let request = match command::parse(body) {
        Ok(request) => request,
        Err(err) => {
            eprintln!("got error {}", err);
            for source in err.sources().skip(1) {
                eprintln!("  Caused by: {source}");
            }

            writer.push_err(ResponseCode::Unknown, "internal error");
            writer.finish();

            return Ok(writer.written());
        }
    };

    let (cmd, args) = (request[0], &request[1..]);

    if cmd == b"get" && args.len() >= 1 {
        do_get(context, &args, &mut writer);
    } else if cmd == b"set" && args.len() >= 2 {
        do_set(context, &args, &mut writer);
    } else if cmd == b"del" && args.len() >= 1 {
        do_del(context, &args, &mut writer);
    } else {
        writer.push_err(
            ResponseCode::Unknown,
            format!("invalid command {}", String::from_utf8_lossy(cmd)),
        );
    }

    writer.finish();
    Ok(writer.written())
}

fn do_get(context: &mut Context, args: &[&[u8]], response_writer: &mut protocol::Writer) {
    println!("do_get; args: {:?}", args);

    let key = match std::str::from_utf8(args[0]) {
        Ok(key) => key,
        Err(_) => {
            response_writer.push_err(ResponseCode::Unknown, "invalid key");
            return;
        }
    };

    match context.data.get(key) {
        None => {
            response_writer.push_nil();
        }
        Some(value) => {
            response_writer.push_string(value);
        }
    }
}

fn do_set(context: &mut Context, args: &[&[u8]], response_writer: &mut protocol::Writer) {
    println!("do_set, args: {:?}", args);

    // TODO(vincent): avoid cloning ?
    let key = match String::from_utf8(args[0].to_vec()) {
        Ok(key) => key,
        Err(_) => {
            response_writer.push_err(ResponseCode::Unknown, "invalid key");
            return;
        }
    };

    let value = match String::from_utf8(args[1].to_vec()) {
        Ok(value) => value,
        Err(_) => {
            response_writer.push_err(ResponseCode::Unknown, "invalid key");
            return;
        }
    };

    context.data.insert(key, value);

    response_writer.push_nil();
}

fn do_del<'b>(context: &mut Context, args: &[&[u8]], response_writer: &mut protocol::Writer) {
    println!("do_del, args: {:?}", args);

    // TODO(vincent): avoid cloning ?
    let key = match std::str::from_utf8(args[0]) {
        Ok(key) => key,
        Err(_) => {
            response_writer.push_err(ResponseCode::Unknown, "invalid key");
            return;
        }
    };

    match context.data.remove(key) {
        None => {
            response_writer.push_int(0);
        }
        Some(_) => {
            response_writer.push_int(1);
        }
    }
}

enum ConnectionAction {
    DoNothing,
    Delete,
}

fn do_read_request(context: &mut Context, connection: &mut Connection) -> ConnectionAction {
    loop {
        let result = match try_fill_buffer(context, connection) {
            Err(err) => {
                match err {
                    TryFillBufferError::EndOfStream => {
                        println!("end of stream for connection {}", connection.fd);
                    }
                    TryFillBufferError::TryOneRequest(err) => {
                        println!("try_one_request call failed, err: {}", err);
                    }
                    TryFillBufferError::IO(err) => {
                        println!("try_fill_buffer call failed, err: {}", err);
                    }
                }
                return ConnectionAction::Delete;
            }
            Ok(v) => v,
        };
        if !result {
            break;
        }
    }

    ConnectionAction::DoNothing
}

fn do_send_responses(connection: &mut Connection) -> ConnectionAction {
    loop {
        let res = match try_flush_buffer(connection) {
            Err(err) => {
                println!("do_send_responses: got error {}", err);

                return ConnectionAction::Delete;
            }
            Ok(v) => v,
        };

        if !res {
            break;
        }
    }

    ConnectionAction::DoNothing
}

fn try_flush_buffer(connection: &mut Connection) -> io::Result<bool> {
    let written = {
        let write_buf = connection.write_buf.readable();

        match shared::write(connection.fd, write_buf) {
            Ok(n) => n,
            Err(err) => {
                if err.raw_os_error().unwrap() != libc::EAGAIN {
                    return Err(err);
                }
                return Ok(false);
            }
        }
    };

    connection.write_buf.update_read_head(written);

    if connection.write_buf.is_empty() {
        // Response was fully sent, change state back

        connection.state = State::ReadRequest;
        connection.write_buf.reset();

        return Ok(false);
    }

    Ok(true)
}

fn accept_new_connection(connections: &mut HashMap<i32, Connection>, fd: i32) -> io::Result<()> {
    // Accept new connection

    let mut client_addr: libc::sockaddr_in = unsafe { mem::zeroed() };
    let mut client_addr_len: libc::socklen_t = unsafe { mem::zeroed() };

    let conn_fd = shared::accept(fd, &mut client_addr, &mut client_addr_len)?;

    println!(
        "accepted connection from {}:{}, fd={}",
        client_addr.sin_addr.s_addr, client_addr.sin_port, conn_fd
    );

    shared::set_socket_nonblocking(conn_fd)?;

    // Create the connection state

    let connection = Connection {
        fd: conn_fd,
        state: State::ReadRequest,
        read_buf: ConnectionBuffer::new(),
        write_buf: ConnectionBuffer::new(),
    };
    connections.insert(conn_fd, connection);

    Ok(())
}

fn main() -> anyhow::Result<()> {
    // Create socket

    let fd = shared::create_socket()?;

    println!("created socket fd={}", fd);

    shared::set_socket_opt(fd, SO_REUSEADDR, 1)?;
    shared::set_socket_nonblocking(fd)?;

    // Bind

    println!("binding socket");

    let addr = shared::make_addr([0, 0, 0, 0], 1234);

    shared::bind(fd, &addr)?;

    // Listen

    println!("listening on 0.0.0.0:1234");

    shared::listen(fd, SOMAXCONN)?;

    // Event loop

    let mut context = Context {
        data: SuperHashMap::new(16),
    };

    let mut connections: HashMap<i32, Connection> = HashMap::new();

    let mut poll_args: Vec<libc::pollfd> = Vec::new();

    loop {
        // Prepare the arguments of the poll

        poll_args.clear();

        // Put the listening fd first
        let pfd = libc::pollfd {
            fd,
            events: POLLIN,
            revents: 0,
        };
        poll_args.push(pfd);

        for (fd, connection) in &connections {
            let pfd = libc::pollfd {
                fd: *fd,
                events: (match connection.state {
                    State::ReadRequest => POLLIN,
                    State::SendResponse => POLLOUT,
                }) | POLLERR,
                revents: 0,
            };
            poll_args.push(pfd);
        }

        // Poll for active fds
        let rv = unsafe {
            libc::poll(
                poll_args.as_mut_ptr(),
                poll_args.len() as libc::nfds_t,
                1000,
            )
        };
        if rv < 0 {
            return Err(std::io::Error::last_os_error().into());
        }

        // Process active connections

        for pfd in &poll_args {
            if pfd.revents <= 0 {
                continue;
            }

            // Try to accept new connections if the listening fd is active
            if pfd.fd == fd {
                accept_new_connection(&mut connections, fd)?;
            } else {
                match connections.get_mut(&pfd.fd) {
                    Some(conn) => {
                        let action = match conn.state {
                            State::ReadRequest => do_read_request(&mut context, conn),
                            State::SendResponse => do_send_responses(conn),
                        };

                        match action {
                            ConnectionAction::DoNothing => {}
                            ConnectionAction::Delete => {
                                connections.remove(&pfd.fd);

                                println!("closing fd={}", pfd.fd);
                                shared::close(pfd.fd)?;
                            }
                        }
                    }
                    None => println!("no connection for fd={}", pfd.fd),
                }
            }
        }
    }
}
