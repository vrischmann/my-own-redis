use libc::{socket, AF_INET, SOCK_STREAM};

pub enum SocketError {
    Unknown(i32),
}

impl From<SocketError> for String {
    fn from(err: SocketError) -> String {
        match err {
            SocketError::Unknown(n) => format!("unknown (code={})", n),
        }
    }
}

pub fn create_socket() -> Result<i32, SocketError> {
    let fd = unsafe { socket(AF_INET, SOCK_STREAM, 0) };
    if fd < 0 {
        Err(SocketError::Unknown(fd))
    } else {
        Ok(fd)
    }
}
