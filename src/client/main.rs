use shared::{BUF_LEN, HEADER_LEN, MAX_MSG_LEN};
use std::mem;

enum QueryError {
    ReadError(shared::ReadError),
    WriteError(shared::WriteError),
    MessageTooLong(usize),
}

impl From<shared::ReadError> for QueryError {
    fn from(err: shared::ReadError) -> QueryError {
        QueryError::ReadError(err)
    }
}

impl From<shared::WriteError> for QueryError {
    fn from(err: shared::WriteError) -> QueryError {
        QueryError::WriteError(err)
    }
}

impl From<QueryError> for String {
    fn from(err: QueryError) -> String {
        match err {
            QueryError::ReadError(err) => err.into(),
            QueryError::WriteError(err) => err.into(),
            QueryError::MessageTooLong(n) => format!("message too long ({} bytes)", n),
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

fn main() -> Result<(), String> {
    // Create socket

    let fd = shared::create_socket()?;

    println!("created socket fd={}", fd);

    // Connect

    let addr = shared::make_addr([127, 0, 0, 1], 1234);

    println!("connecting to 127.0.0.1:1234");

    let rv = unsafe {
        libc::connect(
            fd,
            &addr as *const _ as *const libc::sockaddr,
            mem::size_of_val(&addr) as libc::socklen_t,
        )
    };
    if rv != 0 {
        println!("unable to connect to 127.0.0.1:1234");
        std::process::exit(1);
    }

    println!("connected to 127.0.0.1:1234");

    // Run multiple queries

    query(fd, "hello1")?;
    query(fd, "hello2")?;
    query(fd, "hello3")?;

    unsafe {
        libc::close(fd);
    }

    Ok(())
}
