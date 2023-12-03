use libc::{setsockopt, socket, AF_INET, F_GETFL, F_SETFL, O_NONBLOCK, SOCK_STREAM, SOL_SOCKET};
use onlyerror::Error;
use std::borrow::Cow;
use std::fmt;
use std::io;
use std::mem;

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

pub fn set_socket_nonblocking(fd: i32) -> io::Result<()> {
    let mut flags = unsafe { libc::fcntl(fd, F_GETFL, 0) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }

    flags |= O_NONBLOCK;

    let res = unsafe { libc::fcntl(fd, F_SETFL, flags) };
    if res < 0 {
        return Err(std::io::Error::last_os_error());
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

pub fn read(fd: i32, buf: &mut [u8]) -> io::Result<&[u8]> {
    let n = unsafe { libc::read(fd, buf as *mut _ as *mut libc::c_void, buf.len() - 1) };
    if n < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let data = &buf[0..n as usize];

    Ok(data)
}

#[derive(Error, Debug)]
pub enum ReadFullError {
    #[error("i/o error")]
    IO(#[from] io::Error),
    #[error("end of stream")]
    EndOfStream,
}

pub fn read_full(fd: i32, buf: &mut [u8]) -> Result<(), ReadFullError> {
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
            return Err(ReadFullError::EndOfStream);
        } else if n < 0 {
            return Err(ReadFullError::IO(std::io::Error::last_os_error()));
        }

        let n = n as usize;
        assert!(n <= remaining);

        remaining -= n as usize;
        write_buf = &mut write_buf[n as usize..];
    }

    Ok(())
}

pub fn write(fd: i32, buf: &[u8]) -> io::Result<usize> {
    let n = unsafe { libc::write(fd, buf as *const _ as *const libc::c_void, buf.len()) };
    if n < 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(n as usize)
}

pub fn write_full(fd: i32, buf: &[u8]) -> io::Result<()> {
    let mut remaining = buf.len();
    let mut buf = buf;

    while remaining > 0 {
        let n = unsafe { libc::write(fd, buf as *const _ as *const libc::c_void, buf.len()) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
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
pub const RESPONSE_CODE_LEN: usize = 4;
pub const ARGS_LEN: usize = 4;
pub const STRING_LEN: usize = 4;

#[derive(Debug)]
pub enum Command<'a> {
    Get(Vec<&'a [u8]>),
    Set(Vec<&'a [u8]>),
    Del(Vec<&'a [u8]>),
}

#[derive(Error, Debug)]
pub enum ParseCommandError {
    #[error("input too short")]
    InputTooShort,
    #[error("unknown command '{0}")]
    UnknownCommand(String),
}

impl<'a> Command<'a> {
    pub fn parse(body: &'a [u8]) -> Result<Self, ParseCommandError> {
        let mut body = body;

        if body.len() < ARGS_LEN {
            return Err(ParseCommandError::InputTooShort);
        }

        // 1. Parse the number of arguments.

        let mut n_args = u32::from_be_bytes(body[0..ARGS_LEN].try_into().unwrap());
        // "consume" the bytes we just used
        body = &body[ARGS_LEN..];

        // 2. Parse each argument

        let mut args: Vec<&'a [u8]> = Vec::with_capacity(n_args as usize);
        while n_args > 0 {
            if body.len() <= 0 {
                return Err(ParseCommandError::InputTooShort);
            }

            // An argument is a length-prefixed string:
            // * 4 bytes of length
            // * N bytes of string data

            let string_length = u32::from_be_bytes(body[0..STRING_LEN].try_into().unwrap());

            let arg = &body[STRING_LEN..STRING_LEN + string_length as usize];
            args.push(arg);

            n_args -= 1;

            // "consume" the bytes we just used
            body = &body[STRING_LEN + string_length as usize..];
        }

        // We only care about the first argument for determining the command
        let (cmd, args) = (String::from_utf8_lossy(args[0]), &args[1..]);

        let command = match cmd {
            Cow::Borrowed("get") => Self::Get(args.to_vec()),
            Cow::Borrowed("set") => Self::Set(args.to_vec()),
            Cow::Borrowed("del") => Self::Del(args.to_vec()),
            cmd => return Err(ParseCommandError::UnknownCommand(cmd.to_string())),
        };

        Ok(command)
    }
}

#[derive(Copy, Clone)]
pub enum ResponseCode {
    Ok = 0,
    Err = 1,
    Nx = 2,
}

impl fmt::Display for ResponseCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "OK"),
            Self::Err => write!(f, "ERR"),
            Self::Nx => write!(f, "NX"),
        }
    }
}

impl TryFrom<u32> for ResponseCode {
    type Error = &'static str;

    fn try_from(n: u32) -> Result<Self, Self::Error> {
        match n {
            0 => Ok(Self::Ok),
            1 => Ok(Self::Err),
            2 => Ok(Self::Nx),
            _ => Err("invalid response code"),
        }
    }
}
