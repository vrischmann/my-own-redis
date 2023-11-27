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

fn queries(fd: i32, queries: &[&str]) -> Result<(), QueryError> {
    // Write all

    let write_start = std::time::Instant::now();

    println!("writing all queries: {:?}", queries);

    let mut write_buf = Vec::with_capacity(BUF_LEN * queries.len());
    let mut write_offset = 0;
    for query in queries {
        write_buf.resize(write_buf.len() + (HEADER_LEN + query.len()), 0xaa);
        write_buf[write_offset..write_offset + HEADER_LEN]
            .copy_from_slice(&(query.len() as u32).to_be_bytes());
        write_buf[write_offset + HEADER_LEN..write_offset + (HEADER_LEN + query.len())]
            .copy_from_slice(query.as_bytes());
        write_offset += HEADER_LEN + query.len();
    }

    shared::write_full(fd, &write_buf)?;

    let write_elapsed = std::time::Instant::now() - write_start;

    println!("wrote all queries in {:?}", write_elapsed);

    // Read all

    let read_start = std::time::Instant::now();

    println!("reading all resonses");

    for _ in 0..queries.len() {
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

        shared::read_full(fd, &mut read_buf[HEADER_LEN..HEADER_LEN + message_len])?;
        let body = &read_buf[HEADER_LEN..HEADER_LEN + message_len];

        println!("server says \"{}\"", String::from_utf8_lossy(body));
    }

    let read_elapsed = std::time::Instant::now() - read_start;

    println!("read all responses in {:?}", read_elapsed);

    Ok(())
}

fn query(fd: i32, text: &str) -> Result<(), QueryError> {
    queries(fd, &[text])
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

    queries(fd, &["hello1", "hello2", "hello3"])?;

    // query(fd, "hello1")?;
    // query(fd, "hello2")?;
    // query(fd, "hello3")?;

    shared::close(fd)?;

    Ok(())
}
