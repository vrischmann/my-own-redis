use libc::{SOMAXCONN, SO_REUSEADDR};
use shared::{BUF_LEN, HEADER_LEN, MAX_MSG_LEN};
use std::fmt;
use std::mem;

enum ProcessOneRequestError {
    ReadError(shared::ReadError),
    WriteError(shared::WriteError),
    MessageTooLong(usize),
}

impl fmt::Display for ProcessOneRequestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProcessOneRequestError::ReadError(err) => err.fmt(f),
            ProcessOneRequestError::WriteError(err) => err.fmt(f),
            ProcessOneRequestError::MessageTooLong(n) => {
                write!(f, "message too long ({} bytes)", n)
            }
        }
    }
}

impl From<shared::ReadError> for ProcessOneRequestError {
    fn from(err: shared::ReadError) -> ProcessOneRequestError {
        ProcessOneRequestError::ReadError(err)
    }
}

impl From<shared::WriteError> for ProcessOneRequestError {
    fn from(err: shared::WriteError) -> ProcessOneRequestError {
        ProcessOneRequestError::WriteError(err)
    }
}

fn process_one_request(fd: i32) -> Result<(), ProcessOneRequestError> {
    let mut read_buf: [u8; BUF_LEN] = [0; BUF_LEN];

    // Read and parse request header

    shared::read_full(fd, &mut read_buf[0..HEADER_LEN])?;
    let message_len = {
        let header_data = &read_buf[0..HEADER_LEN];
        let len = i32::from_be_bytes(header_data.try_into().unwrap());

        len as usize
    };

    if message_len > MAX_MSG_LEN {
        return Err(ProcessOneRequestError::MessageTooLong(message_len));
    }

    // Read request body

    shared::read_full(fd, &mut read_buf[HEADER_LEN..])?;
    let body = &read_buf[HEADER_LEN..];

    println!("client says \"{}\"", String::from_utf8_lossy(body));

    // Reply

    let reply = "world";

    let mut write_buf: [u8; BUF_LEN] = [0; BUF_LEN];

    write_buf[0..HEADER_LEN].copy_from_slice(&(reply.len() as u32).to_be_bytes());
    write_buf[HEADER_LEN..HEADER_LEN + reply.len()].copy_from_slice(reply.as_bytes());

    shared::write_full(fd, &write_buf)?;

    Ok(())
}

enum State {
    ReadRequest,
    SendResponse,
    DeleteConnection,
}

struct Connection {
    fd: i32,
    state: State,

    read_buf_size: usize,
    read_buf: [u8; shared::BUF_LEN],

    write_buf_size: usize,
    write_buf_sent: usize,
    write_buf: [u8; shared::BUF_LEN],
}

fn main() -> Result<(), shared::MainError> {
    // Create socket

    let fd = shared::create_socket()?;

    println!("created socket fd={}", fd);

    shared::set_socket_opt(fd, SO_REUSEADDR, 1)?;

    // Bind

    println!("binding socket");

    let addr = shared::make_addr([0, 0, 0, 0], 1234);

    shared::bind(fd, &addr)?;

    // Listen

    println!("listening on 0.0.0.0:1234");

    shared::listen(fd, SOMAXCONN)?;

    loop {
        let mut client_addr: libc::sockaddr_in = unsafe { mem::zeroed() };
        let mut client_addr_len: libc::socklen_t = unsafe { mem::zeroed() };

        let conn_fd = shared::accept(fd, &mut client_addr, &mut client_addr_len)?;

        println!(
            "accepted connection from {}:{}, fd={}",
            client_addr.sin_addr.s_addr, client_addr.sin_port, conn_fd
        );

        // serve this connection indefinitely

        loop {
            if let Err(err) = process_one_request(conn_fd) {
                println!("failed to process request, err: {}", err);
                break;
            }
        }

        println!("closing fd={}", conn_fd);

        shared::close(conn_fd)?;
    }
}
