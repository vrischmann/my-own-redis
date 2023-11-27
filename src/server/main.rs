use libc::{POLLERR, POLLIN, POLLOUT};
use libc::{SOMAXCONN, SO_REUSEADDR};
use shared::{Command, ResponseCode, BUF_LEN, HEADER_LEN, MAX_MSG_LEN, RESPONSE_CODE_LEN};
use std::collections::HashMap;
use std::fmt;
use std::io;
use std::mem;

#[derive(Debug)]
enum State {
    ReadRequest,
    SendResponse,
}

struct Connection {
    fd: i32,
    state: State,

    read_buf_size: usize,
    read_buf_consumed: usize,
    read_buf: [u8; BUF_LEN],

    write_buf_size: usize,
    write_buf_sent: usize,
    write_buf: [u8; BUF_LEN],
}

enum TryFillBufferError {
    TryOneRequest(TryOneRequestError),
    IO(io::Error),
    EndOfStream,
}

impl From<TryOneRequestError> for TryFillBufferError {
    fn from(err: TryOneRequestError) -> Self {
        TryFillBufferError::TryOneRequest(err)
    }
}

impl From<io::Error> for TryFillBufferError {
    fn from(err: io::Error) -> Self {
        TryFillBufferError::IO(err)
    }
}

impl fmt::Display for TryFillBufferError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::TryOneRequest(err) => err.fmt(f),
            Self::IO(err) => err.fmt(f),
            Self::EndOfStream => write!(f, "end of stream"),
        }
    }
}

fn try_fill_buffer(connection: &mut Connection) -> Result<bool, TryFillBufferError> {
    assert!(connection.read_buf_size < connection.read_buf.len());

    // Remove the already processed requests from the buffer, if any

    if connection.read_buf_size > 0 {
        let remaining = connection.read_buf_size - connection.read_buf_consumed;
        if remaining > 0 {
            let next_request_start = connection.read_buf_consumed;

            println!(
                "move bytes from {:?} to the start of the read buf",
                next_request_start..next_request_start + remaining
            );

            connection
                .read_buf
                .copy_within(next_request_start..next_request_start + remaining, 0);

            connection.read_buf_size = remaining;
        }
    }

    connection.read_buf_consumed = 0;

    //

    let data = loop {
        let read_buf = &mut connection.read_buf[connection.read_buf_size..];

        match shared::read(connection.fd, read_buf) {
            Ok(data) => {
                if data.is_empty() {
                    return Err(TryFillBufferError::EndOfStream);
                } else {
                    break data;
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

    connection.read_buf_size += data.len();
    assert!(connection.read_buf_size < connection.read_buf.len());

    // Try to process requests
    loop {
        if !try_one_request(connection)? {
            break;
        }
    }

    // Try to send the responses

    connection.state = State::SendResponse;
    do_send_responses(connection).unwrap();

    if let State::ReadRequest = connection.state {
        Ok(true)
    } else {
        Ok(false)
    }
}

enum TryOneRequestError {
    SendResponse(SendResponseError),
    DoRequest(DoRequestError),
    MessageTooLong,
}

impl From<SendResponseError> for TryOneRequestError {
    fn from(err: SendResponseError) -> Self {
        TryOneRequestError::SendResponse(err)
    }
}

impl From<DoRequestError> for TryOneRequestError {
    fn from(err: DoRequestError) -> Self {
        TryOneRequestError::DoRequest(err)
    }
}

impl fmt::Display for TryOneRequestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::SendResponse(err) => err.fmt(f),
            Self::MessageTooLong => write!(f, "message too long"),
            Self::DoRequest(err) => err.fmt(f),
        }
    }
}

fn try_one_request(connection: &mut Connection) -> Result<bool, TryOneRequestError> {
    // Parse the request

    if connection.read_buf_size < HEADER_LEN {
        return Ok(false);
    }

    let read_buf = &connection.read_buf[connection.read_buf_consumed..];

    let message_len = {
        let header_data = &read_buf[0..HEADER_LEN];
        let len = u32::from_be_bytes(header_data.try_into().unwrap());

        len as usize
    };
    if message_len > MAX_MSG_LEN {
        return Err(TryOneRequestError::MessageTooLong);
    }

    if HEADER_LEN + message_len > connection.read_buf_size {
        // Not enough data in the buffer
        return Ok(false);
    }

    // "consume" the bytes of the current request
    connection.read_buf_size -= HEADER_LEN + message_len;
    connection.read_buf_consumed += HEADER_LEN + message_len;

    // Process the request

    let request_body = &read_buf[HEADER_LEN..HEADER_LEN + message_len];
    let write_buf = &mut connection.write_buf[connection.write_buf_size..];

    let (response_code, written) = do_request(
        request_body,
        &mut write_buf[HEADER_LEN + RESPONSE_CODE_LEN..],
    )?;

    write_buf[0..HEADER_LEN].copy_from_slice(&(written as u32).to_be_bytes());
    write_buf[HEADER_LEN..HEADER_LEN + RESPONSE_CODE_LEN]
        .copy_from_slice(&(response_code as u32).to_be_bytes());
    connection.write_buf_size += HEADER_LEN + RESPONSE_CODE_LEN + written as usize;

    println!(
        "write buf in try_one_request: {:?}",
        &write_buf[0..connection.write_buf_size]
    );

    // Continue the outer loop if the request was fully processed
    match connection.state {
        State::ReadRequest => Ok(true),
        _ => Ok(false),
    }
}

enum DoRequestError {
    ParseRequest(shared::ParseCommandError),
}

impl From<shared::ParseCommandError> for DoRequestError {
    fn from(err: shared::ParseCommandError) -> Self {
        Self::ParseRequest(err)
    }
}

impl fmt::Display for DoRequestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ParseRequest(err) => err.fmt(f),
        }
    }
}

fn do_request(body: &[u8], write_buf: &mut [u8]) -> Result<(ResponseCode, usize), DoRequestError> {
    println!("client says {:?}", body);

    let request = match Command::parse(body) {
        Ok(request) => request,
        Err(err) => {
            println!("got error {}", err);

            let resp = "Unknown command";
            write_buf[0..resp.len()].copy_from_slice(resp.as_bytes());

            return Ok((ResponseCode::Err, resp.len()));
        }
    };

    let response_code = match request {
        Command::Get(args) => do_get(&args)?,
        Command::Set(args) => do_set(&args)?,
        Command::Del(args) => do_del(&args)?,
    };

    let resp = format!("foobar{}", String::from_utf8_lossy(body));
    write_buf[0..resp.len()].copy_from_slice(resp.as_bytes());

    Ok((response_code, resp.len()))
}

fn do_get(args: &[&[u8]]) -> Result<ResponseCode, DoRequestError> {
    println!("do_get, args: {:?}", args);

    Ok(ResponseCode::Ok)
}

fn do_set(args: &[&[u8]]) -> Result<ResponseCode, DoRequestError> {
    println!("do_set, args: {:?}", args);

    Ok(ResponseCode::Ok)
}

fn do_del(args: &[&[u8]]) -> Result<ResponseCode, DoRequestError> {
    println!("do_del, args: {:?}", args);

    Ok(ResponseCode::Ok)
}

enum ReadRequestError {
    TryFillBuffer(TryFillBufferError),
}

impl From<TryFillBufferError> for ReadRequestError {
    fn from(err: TryFillBufferError) -> Self {
        ReadRequestError::TryFillBuffer(err)
    }
}

impl fmt::Display for ReadRequestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::TryFillBuffer(err) => err.fmt(f),
        }
    }
}

enum ConnectionAction {
    DoNothing,
    Delete,
}

fn do_read_request(connection: &mut Connection) -> Result<ConnectionAction, ReadRequestError> {
    loop {
        let result = match try_fill_buffer(connection) {
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
                return Ok(ConnectionAction::Delete);
            }
            Ok(v) => v,
        };
        if !result {
            break;
        }
    }

    Ok(ConnectionAction::DoNothing)
}

#[derive(Debug)]
enum SendResponseError {
    TryFlushBuffer(TryFlushBufferError),
}

impl From<TryFlushBufferError> for SendResponseError {
    fn from(err: TryFlushBufferError) -> Self {
        Self::TryFlushBuffer(err)
    }
}

impl fmt::Display for SendResponseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::TryFlushBuffer(err) => err.fmt(f),
        }
    }
}

fn do_send_responses(connection: &mut Connection) -> Result<ConnectionAction, SendResponseError> {
    loop {
        if !try_flush_buffer(connection)? {
            break;
        }
    }

    Ok(ConnectionAction::DoNothing)
}

#[derive(Debug)]
enum TryFlushBufferError {
    IO(io::Error),
}

impl From<io::Error> for TryFlushBufferError {
    fn from(err: io::Error) -> Self {
        TryFlushBufferError::IO(err)
    }
}

impl fmt::Display for TryFlushBufferError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::IO(err) => err.fmt(f),
        }
    }
}

fn try_flush_buffer(connection: &mut Connection) -> Result<bool, TryFlushBufferError> {
    let written = loop {
        let write_buf =
            &mut connection.write_buf[connection.write_buf_sent..connection.write_buf_size];

        match shared::write(connection.fd, write_buf) {
            Ok(n) => break n,
            Err(err) => {
                if err.raw_os_error().unwrap() != libc::EAGAIN {
                    return Err(TryFlushBufferError::IO(err));
                }
                return Ok(false);
            }
        }
    };

    connection.write_buf_sent += written;
    assert!(connection.write_buf_sent <= connection.write_buf_size);

    if connection.write_buf_sent == connection.write_buf_size {
        // Response was fully sent, change state back

        connection.state = State::ReadRequest;
        connection.write_buf_size = 0;
        connection.write_buf_sent = 0;

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
        read_buf_size: 0,
        read_buf_consumed: 0,
        read_buf: [0; BUF_LEN],
        write_buf_size: 0,
        write_buf_sent: 0,
        write_buf: [0; BUF_LEN],
    };
    connections.insert(conn_fd, connection);

    Ok(())
}

fn main() -> Result<(), shared::MainError> {
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
                            State::ReadRequest => do_read_request(conn)?,
                            State::SendResponse => do_send_responses(conn)?,
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
