use libc::{setsockopt, socket, AF_INET, F_GETFL, F_SETFL, O_NONBLOCK, SOCK_STREAM, SOL_SOCKET};
use std::fmt;
use std::io;
use std::mem;

#[derive(Debug)]
pub struct MainError {
    message: String,
}

impl<T> From<T> for MainError
where
    T: fmt::Display,
{
    fn from(v: T) -> Self {
        Self {
            message: v.to_string(),
        }
    }
}

//

pub fn make_addr(addr: [u8; 4], port: u16) -> libc::sockaddr_in {
    let s_addr = u32::from_be_bytes(addr);

    libc::sockaddr_in {
        sin_family: AF_INET as libc::sa_family_t,
        sin_port: port.to_be(),
        sin_addr: libc::in_addr {
            s_addr: s_addr.to_be(),
        },
        sin_zero: [0; 8],
        #[cfg(target_os = "macos")]
        sin_len: 0,
    }
}

pub fn create_socket() -> io::Result<i32> {
    let fd = unsafe { socket(AF_INET, SOCK_STREAM, 0) };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

pub enum FnctlError {
    IO(io::Error),
}

impl From<FnctlError> for String {
    fn from(err: FnctlError) -> String {
        match err {
            FnctlError::IO(err) => format!("{}", err),
        }
    }
}

pub fn set_socket_nonblocking(fd: i32) -> Result<(), FnctlError> {
    let mut flags = unsafe { libc::fcntl(fd, F_GETFL, 0) };
    if flags < 0 {
        return Err(FnctlError::IO(std::io::Error::last_os_error()));
    }

    flags |= O_NONBLOCK;

    let res = unsafe { libc::fcntl(fd, F_SETFL, flags) };
    if res < 0 {
        return Err(FnctlError::IO(std::io::Error::last_os_error()));
    }

    Ok(())
}

pub fn set_socket_opt(fd: i32, opt: libc::c_int, val: i32) -> io::Result<()> {
    let n = unsafe {
        setsockopt(
            fd,
            SOL_SOCKET,
            opt,
            &val as *const _ as *const libc::c_void,
            mem::size_of_val(&val) as libc::socklen_t,
        )
    };
    if n < 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

pub fn bind(fd: i32, addr: &libc::sockaddr_in) -> io::Result<()> {
    let rv = unsafe {
        libc::bind(
            fd,
            addr as *const _ as *const libc::sockaddr,
            mem::size_of_val(addr) as libc::socklen_t,
        )
    };
    if rv < 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

pub fn listen(fd: i32, backlog: libc::c_int) -> io::Result<()> {
    let rv = unsafe { libc::listen(fd, backlog) };
    if rv < 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

pub fn accept(
    fd: i32,
    addr: &mut libc::sockaddr_in,
    addr_len: &mut libc::socklen_t,
) -> io::Result<i32> {
    let conn_fd = unsafe { libc::accept(fd, addr as *mut _ as *mut libc::sockaddr, addr_len) };
    if conn_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(conn_fd)
}

pub fn close(fd: i32) -> io::Result<()> {
    let n = unsafe { libc::close(fd) };
    if n < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub fn connect(fd: i32, addr: &libc::sockaddr_in) -> io::Result<()> {
    let n = unsafe {
        libc::connect(
            fd,
            addr as *const _ as *const libc::sockaddr,
            mem::size_of_val(addr) as libc::socklen_t,
        )
    };
    if n < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub enum ReadError {
    EndOfStream,
    IO(io::Error),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ReadError::EndOfStream => write!(f, "end of stream"),
            ReadError::IO(err) => err.fmt(f),
        }
    }
}

pub fn read(fd: i32, buf: &mut [u8]) -> Result<&[u8], ReadError> {
    let n = unsafe { libc::read(fd, buf as *mut _ as *mut libc::c_void, buf.len() - 1) };
    if n < 0 {
        return Err(ReadError::IO(std::io::Error::last_os_error()));
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
        if n == 0 {
            return Err(ReadError::EndOfStream);
        } else if n < 0 {
            return Err(ReadError::IO(std::io::Error::last_os_error()));
        }

        let n = n as usize;
        assert!(n <= remaining);

        remaining -= n as usize;
        write_buf = &mut write_buf[n as usize..];
    }

    Ok(())
}

pub enum WriteError {
    IO(io::Error),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            WriteError::IO(err) => err.fmt(f),
        }
    }
}

pub fn write(fd: i32, buf: &[u8]) -> Result<isize, WriteError> {
    let n = unsafe { libc::write(fd, buf as *const _ as *const libc::c_void, buf.len()) };
    if n < 0 {
        return Err(WriteError::IO(std::io::Error::last_os_error()));
    }

    Ok(n)
}

pub fn write_full(fd: i32, buf: &[u8]) -> Result<(), WriteError> {
    let mut remaining = buf.len();
    let mut buf = buf;

    while remaining > 0 {
        let n = unsafe { libc::write(fd, buf as *const _ as *const libc::c_void, buf.len()) };
        if n < 0 {
            return Err(WriteError::IO(std::io::Error::last_os_error()));
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
