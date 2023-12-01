use libc::{POLLERR, POLLIN, POLLOUT};
use libc::{SOMAXCONN, SO_REUSEADDR};
use shared::{
    Command, ResponseCode, BUF_LEN, HEADER_LEN, MAX_MSG_LEN, RESPONSE_CODE_LEN, STRING_LEN,
};
use std::collections::HashMap;
use std::fmt;
use std::io;
use std::mem;

struct Context {
    data: HashMap<String, String>,
}

#[derive(Debug)]
enum State {
    ReadRequest,
    SendResponse,
}

struct ResponseWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> ResponseWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf,
            pos: HEADER_LEN + RESPONSE_CODE_LEN,
        }
    }

    fn finish(&mut self) {
        let buf = &mut self.buf[0..HEADER_LEN];

        let written = self.pos - HEADER_LEN;

        buf.copy_from_slice(&(written as u32).to_be_bytes());
    }

    fn set_response_code(&mut self, code: ResponseCode) {
        let buf = &mut self.buf[HEADER_LEN..HEADER_LEN + RESPONSE_CODE_LEN];
        buf.copy_from_slice(&(code as u32).to_be_bytes());
    }

    fn push_string<T: AsRef<[u8]>>(&mut self, value: T) {
        let buf = &mut self.buf[self.pos..];

        let bytes = value.as_ref();

        buf[0..STRING_LEN].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        buf[STRING_LEN..STRING_LEN + bytes.len()].copy_from_slice(bytes);

        self.pos += STRING_LEN + bytes.len()
    }

    fn written(&self) -> usize {
        self.pos
    }
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

    // fn push(&mut self, data: &mut [u8]) {
    //     let buf = self.writable();
    //     buf.copy_from_slice(data);
    //
    //     self.update_write_head(data.len());
    // }

    fn readable(&self) -> &[u8] {
        &self.data[self.read_head..self.write_head]
    }

    fn update_write_head(&mut self, n: usize) {
        self.write_head += n;
        assert!(self.write_head < self.data.len());
    }

    fn update_read_head(&mut self, n: usize) {
        self.read_head += n
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
    do_send_responses(context, connection).unwrap();

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

fn try_one_request(
    context: &mut Context,
    connection: &mut Connection,
) -> Result<bool, TryOneRequestError> {
    // Parse the request

    let request_body = {
        let buf = connection.read_buf.readable();

        if buf.len() < HEADER_LEN {
            return Ok(false);
        }

        let message_len = {
            let header_data = &buf[0..HEADER_LEN];
            let len = u32::from_be_bytes(header_data.try_into().unwrap());

            len as usize
        };
        println!(
            "buf: {:?} ({}), message len: {}",
            buf,
            String::from_utf8_lossy(buf),
            message_len
        );
        if message_len > MAX_MSG_LEN {
            return Err(TryOneRequestError::MessageTooLong);
        }

        if buf.len() < HEADER_LEN + message_len {
            // Not enough data in the buffer
            return Ok(false);
        }

        &buf[HEADER_LEN..HEADER_LEN + message_len]
    };

    println!(
        "request body: {:?} ({})",
        request_body,
        String::from_utf8_lossy(request_body),
    );

    // Process the request
    {
        let written = {
            let mut response_writer = ResponseWriter::new(connection.write_buf.writable());

            do_request(context, request_body, &mut response_writer)?;

            response_writer.finish();
            response_writer.written()
        };

        connection.write_buf.update_write_head(written);

        println!(
            "write buf in try_one_request: {:?}",
            connection.write_buf.readable()
        );
    }

    // "consume" the bytes of the current request
    connection
        .read_buf
        .update_read_head(HEADER_LEN + request_body.len());

    println!(
        "buf after: {:?} ({})",
        &connection.read_buf.readable(),
        String::from_utf8_lossy(&connection.read_buf.readable()),
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

fn do_request(
    context: &mut Context,
    body: &[u8],
    response_writer: &mut ResponseWriter,
) -> Result<(), DoRequestError> {
    println!("client says {:?}", body);

    let request = match Command::parse(body) {
        Ok(request) => request,
        Err(err) => {
            println!("got error {}", err);

            let resp = "Unknown command";

            response_writer.set_response_code(ResponseCode::Err);
            response_writer.push_string(resp);

            return Ok(());
        }
    };

    let mut response_buf: [u8; MAX_MSG_LEN - RESPONSE_CODE_LEN] =
        [0; MAX_MSG_LEN - RESPONSE_CODE_LEN];

    let (response_code, response) = match request {
        Command::Get(args) => do_get(context, &args, &mut response_buf)?,
        Command::Set(args) => do_set(context, &args, &mut response_buf)?,
        Command::Del(args) => do_del(context, &args, &mut response_buf)?,
    };

    response_writer.set_response_code(response_code);
    if response.len() > 0 {
        response_writer.push_string(response);
    }

    Ok(())
}

fn do_get<'b>(
    context: &mut Context,
    args: &[&[u8]],
    buf: &'b mut [u8],
) -> Result<(ResponseCode, &'b [u8]), DoRequestError> {
    println!("do_get; args: {:?}", args);

    if args.len() <= 0 {
        let resp = "no key provided";
        buf[0..resp.len()].copy_from_slice(resp.as_bytes());

        let response = &buf[0..resp.len()];

        return Ok((ResponseCode::Err, response));
    }

    let key = match std::str::from_utf8(args[0]) {
        Ok(key) => key,
        Err(_) => {
            let resp = "invalid key";
            buf[0..resp.len()].copy_from_slice(resp.as_bytes());

            let response = &buf[0..resp.len()];

            return Ok((ResponseCode::Err, response));
        }
    };

    match context.data.get(key) {
        None => {
            println!("do_get; no value for key {}", key);

            Ok((ResponseCode::Nx, b""))
        }
        Some(value) => {
            println!("do_get; value for key {}: {}", key, value);

            assert!(value.len() < buf.len());

            buf[0..value.len()].copy_from_slice(value.as_bytes());
            let response = &buf[0..buf.len()];

            Ok((ResponseCode::Ok, response))
        }
    }
}

fn do_set<'b>(
    context: &mut Context,
    args: &[&[u8]],
    response: &'b mut [u8],
) -> Result<(ResponseCode, &'b [u8]), DoRequestError> {
    println!("do_set, args: {:?}", args);

    Ok((ResponseCode::Ok, b""))
}

fn do_del<'b>(
    context: &mut Context,
    args: &[&[u8]],
    response: &'b mut [u8],
) -> Result<(ResponseCode, &'b [u8]), DoRequestError> {
    println!("do_del, args: {:?}", args);

    Ok((ResponseCode::Ok, b""))
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

fn do_read_request(
    context: &mut Context,
    connection: &mut Connection,
) -> Result<ConnectionAction, ReadRequestError> {
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

fn do_send_responses(
    context: &mut Context,
    connection: &mut Connection,
) -> Result<ConnectionAction, SendResponseError> {
    loop {
        if !try_flush_buffer(context, connection)? {
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

fn try_flush_buffer(
    context: &mut Context,
    connection: &mut Connection,
) -> Result<bool, TryFlushBufferError> {
    let written = loop {
        let write_buf = connection.write_buf.readable();

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

    let mut context = Context {
        data: HashMap::new(),
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
                            State::ReadRequest => do_read_request(&mut context, conn)?,
                            State::SendResponse => do_send_responses(&mut context, conn)?,
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
