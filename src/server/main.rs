use hash_map::SuperHashMap;
use libc::{POLLERR, POLLIN, POLLOUT};
use libc::{SOMAXCONN, SO_REUSEADDR};
use onlyerror::Error;
use shared::protocol::BUF_LEN;
use shared::ResponseCode;
use shared::{command, protocol};
use std::collections::HashMap;
use std::io;
use std::mem;

mod hash_map;

struct Context {
    data: SuperHashMap<String, String>,
}

#[derive(Debug)]
enum State {
    ReadRequest,
    SendResponse,
}

struct ConnectionBuffer {
    data: Vec<u8>,
    write_head: usize,
    read_head: usize,
}

impl ConnectionBuffer {
    fn new() -> Self {
        let mut data = Vec::with_capacity(BUF_LEN);
        data.resize(BUF_LEN, 0xaa);

        Self {
            data,
            write_head: 0,
            read_head: 0,
        }
    }

    fn writable(&mut self) -> &mut [u8] {
        &mut self.data[self.write_head..]
    }

    fn readable(&self) -> &[u8] {
        &self.data[self.read_head..self.write_head]
    }

    fn update_write_head(&mut self, n: usize) {
        self.write_head += n;
        assert!(self.write_head < self.data.len());
    }

    fn update_read_head(&mut self, n: usize) {
        self.read_head += n;
        assert!(self.read_head <= self.write_head);
    }

    fn remove_processed(&mut self) {
        let remaining = self.write_head - self.read_head;
        if remaining <= 0 {
            return;
        }

        let next = self.read_head;

        println!(
            "move bytes from {:?} to the start of the read buf",
            next..next + remaining
        );

        self.data.copy_within(next..next + remaining, 0);
        self.read_head = 0;
    }
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
            protocol::Error::MessageTooLong(_) => return Err(err.into()),
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
            println!("got error {}", err);

            let resp = "Unknown command";

            writer.push_u32(ResponseCode::Err);
            writer.push_string(resp);

            writer.finish();
            return Ok(writer.written());
        }
    };

    let (cmd, args) = (request[0], &request[1..]);

    match cmd {
        b"get" => do_get(context, &args, &mut writer),
        b"set" => do_set(context, &args, &mut writer),
        b"del" => do_del(context, &args, &mut writer),
        _ => panic!(
            "unknown command {}, should never happen",
            String::from_utf8_lossy(cmd)
        ),
    }

    writer.finish();
    Ok(writer.written())
}

fn do_get(context: &mut Context, args: &[&[u8]], response_writer: &mut protocol::Writer) {
    println!("do_get; args: {:?}", args);

    if args.len() <= 0 {
        let resp = "no key provided";

        response_writer.push_u32(ResponseCode::Err);
        response_writer.push_string(resp);

        return;
    }

    let key = match std::str::from_utf8(args[0]) {
        Ok(key) => key,
        Err(_) => {
            let resp = "invalid key";

            response_writer.push_u32(ResponseCode::Err);
            response_writer.push_string(resp);

            return;
        }
    };

    match context.data.get(key) {
        None => {
            response_writer.push_u32(ResponseCode::Nx);
        }
        Some(value) => {
            response_writer.push_u32(ResponseCode::Ok);
            response_writer.push_string(value);
        }
    }
}

fn do_set(context: &mut Context, args: &[&[u8]], response_writer: &mut protocol::Writer) {
    println!("do_set, args: {:?}", args);

    if args.len() != 2 {
        let resp = "no key and value provided";

        response_writer.push_u32(ResponseCode::Err);
        response_writer.push_string(resp);

        return;
    }

    // TODO(vincent): avoid cloning ?
    let key = match String::from_utf8(args[0].to_vec()) {
        Ok(key) => key,
        Err(_) => {
            let resp = "invalid key";

            response_writer.push_u32(ResponseCode::Err);
            response_writer.push_string(resp);

            return;
        }
    };

    let value = match String::from_utf8(args[1].to_vec()) {
        Ok(value) => value,
        Err(_) => {
            let resp = "invalid key";

            response_writer.push_u32(ResponseCode::Err);
            response_writer.push_string(resp);

            return;
        }
    };

    context.data.insert(key, value);

    response_writer.push_u32(ResponseCode::Ok);
}

fn do_del<'b>(context: &mut Context, args: &[&[u8]], response_writer: &mut protocol::Writer) {
    println!("do_del, args: {:?}", args);

    if args.len() != 1 {
        let resp = "no key provided";

        response_writer.push_u32(ResponseCode::Err);
        response_writer.push_string(resp);

        return;
    }

    // TODO(vincent): avoid cloning ?
    let key = match std::str::from_utf8(args[0]) {
        Ok(key) => key,
        Err(_) => {
            let resp = "invalid key";

            response_writer.push_u32(ResponseCode::Err);
            response_writer.push_string(resp);

            return;
        }
    };

    match context.data.remove(key) {
        None => {
            response_writer.push_u32(ResponseCode::Nx);
        }
        Some(_) => {
            response_writer.push_u32(ResponseCode::Ok);
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
                println!("got error {}", err);

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
    let written = loop {
        let write_buf = connection.write_buf.readable();

        match shared::write(connection.fd, write_buf) {
            Ok(n) => break n,
            Err(err) => {
                if err.raw_os_error().unwrap() != libc::EAGAIN {
                    return Err(err);
                }
                return Ok(false);
            }
        }
    };

    connection.write_buf.update_read_head(written);

    if connection.write_buf.read_head == connection.write_buf.write_head {
        // Response was fully sent, change state back

        connection.state = State::ReadRequest;
        connection.write_buf.read_head = 0;
        connection.write_buf.write_head = 0;

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
