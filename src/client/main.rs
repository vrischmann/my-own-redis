use shared::{BUF_LEN, HEADER_LEN, MAX_MSG_LEN};
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

fn query(fd: i32, text: &str) -> Result<(), QueryError> {
    // Write

    let mut write_buf: [u8; BUF_LEN] = [0; BUF_LEN];

    write_buf[0..HEADER_LEN].copy_from_slice(&(text.len() as u32).to_be_bytes());
    write_buf[HEADER_LEN..HEADER_LEN + text.len()].copy_from_slice(text.as_bytes());

    shared::write_full(fd, &write_buf)?;

    // Read

    let mut read_buf: [u8; BUF_LEN] = [0; BUF_LEN];

    shared::read_full(fd, &mut read_buf[0..HEADER_LEN])?;
    let message_len = {
        let header_data = &read_buf[0..HEADER_LEN];

        let len = i32::from_be_bytes(header_data.try_into().unwrap());

        len as usize
    };

    if message_len > MAX_MSG_LEN {
        return Err(QueryError::MessageTooLong(message_len));
    }

    // Read request body

    shared::read_full(fd, &mut read_buf[HEADER_LEN..])?;
    let body = &read_buf[HEADER_LEN..];

    println!("server says \"{}\"", String::from_utf8_lossy(body));

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

    query(fd, "hello1")?;
    query(fd, "hello2")?;
    query(fd, "hello3")?;

    shared::close(fd)?;

    Ok(())
}
