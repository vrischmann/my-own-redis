use libc::{socket, AF_INET, SOCK_STREAM};

pub fn make_addr(addr: [u8; 4], port: u16) -> libc::sockaddr_in {
    let s_addr = u32::from_be_bytes(addr);

    libc::sockaddr_in {
        sin_family: AF_INET as libc::sa_family_t,
        sin_port: port.to_be(),
        sin_addr: libc::in_addr {
            s_addr: s_addr.to_be(),
        },
        sin_zero: [0; 8],
    }
}

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

pub enum ReadError {
    Unknown(i32),
}

impl From<ReadError> for String {
    fn from(err: ReadError) -> String {
        match err {
            ReadError::Unknown(n) => format!("unknown (code={})", n),
        }
    }
}

pub fn read(fd: i32, buf: &mut [u8]) -> Result<&[u8], ReadError> {
    let n = unsafe { libc::read(fd, buf as *mut _ as *mut libc::c_void, buf.len()) };
    if n < 0 {
        return Err(ReadError::Unknown(fd));
    }

    let data = &buf[0..n as usize];

    Ok(data)
}

pub fn read_full(fd: i32, buf: &mut [u8]) -> Result<(), ReadError> {
    let mut remaining = buf.len();
    let mut write_buf = buf;

    while remaining > 0 {
        let n = unsafe {
            libc::read(
                fd,
                write_buf as *mut _ as *mut libc::c_void,
                remaining as usize,
            )
        };
        if n < 0 {
            return Err(ReadError::Unknown(fd));
        }

        let n = n as usize;
        assert!(n <= remaining);

        remaining -= n as usize;
        write_buf = &mut write_buf[n as usize..];
    }

    Ok(())
}

pub enum WriteError {
    Unknown(i32),
}

impl From<WriteError> for String {
    fn from(err: WriteError) -> String {
        match err {
            WriteError::Unknown(n) => format!("unknown (code={})", n),
        }
    }
}

pub fn write(fd: i32, buf: &[u8]) -> Result<isize, WriteError> {
    let n = unsafe { libc::write(fd, buf as *const _ as *const libc::c_void, buf.len()) };
    if n < 0 {
        return Err(WriteError::Unknown(n as i32));
    }

    Ok(n)
}

pub fn write_full(fd: i32, buf: &[u8]) -> Result<(), WriteError> {
    let mut remaining = buf.len();
    let mut buf = buf;

    while remaining > 0 {
        let n = unsafe { libc::write(fd, buf as *const _ as *const libc::c_void, buf.len()) };
        if n < 0 {
            return Err(WriteError::Unknown(n as i32));
        }

        let n = n as usize;
        assert!(n <= remaining);

        remaining -= n as usize;
        buf = &buf[n as usize..];
    }

    Ok(())
}

pub const HEADER_LEN: usize = 4;
pub const MAX_MSG_LEN: usize = 4096;
pub const BUF_LEN: usize = HEADER_LEN + MAX_MSG_LEN;

pub struct Request<'a> {
    pub body: &'a [u8],
}

pub enum ParseRequestError {
    Incomplete,
    MessageTooLong(usize),
}

impl<'a> Request<'a> {
    pub fn try_parse(buf: &'a [u8]) -> Result<(Self, usize), ParseRequestError> {
        if buf.len() < HEADER_LEN {
            return Err(ParseRequestError::Incomplete);
        }

        let message_len = {
            let header_data = &buf[0..HEADER_LEN];
            let len = i32::from_be_bytes(header_data.try_into().unwrap());

            len as usize
        };

        if message_len > MAX_MSG_LEN {
            return Err(ParseRequestError::MessageTooLong(message_len));
        }

        let body = &buf[HEADER_LEN..];
        if body.len() < message_len {
            return Err(ParseRequestError::Incomplete);
        }

        let consumed = HEADER_LEN + body.len();

        Ok((Self { body }, consumed))
    }
}
