use libc::setsockopt;
use libc::{SOL_SOCKET, SOMAXCONN, SO_REUSEADDR};
use shared::{BUF_LEN, HEADER_LEN};
use std::mem;

enum ProcessOneRequestError {
    ReadError(shared::ReadError),
    WriteError(shared::WriteError),
    EndOfStream,
}

impl From<ProcessOneRequestError> for String {
    fn from(err: ProcessOneRequestError) -> String {
        match err {
            ProcessOneRequestError::ReadError(err) => err.into(),
            ProcessOneRequestError::WriteError(err) => err.into(),
            ProcessOneRequestError::EndOfStream => "end of stream".to_string(),
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

fn process_requests(fd: i32) -> Result<(), ProcessOneRequestError> {
    let mut read_buf: [u8; BUF_LEN] = [0; BUF_LEN];
    let mut read_pos = 0;
    let mut need_more = true;

    loop {
        if need_more {
            let data = shared::read(fd, &mut read_buf[read_pos..])?;
            if data.is_empty() {
                return Err(ProcessOneRequestError::EndOfStream);
            }

            read_pos += data.len();
            need_more = false;

            println!("read {} bytes of data", data.len());
        }

        let data = &read_buf[0..read_pos];

        match shared::Request::try_parse(&data) {
            Ok((request, consumed)) => {
                println!("client says \"{}\"", String::from_utf8_lossy(request.body));

                // Reply

                let reply = "world";

                let mut write_buf: [u8; BUF_LEN] = [0; BUF_LEN];

                write_buf[0..HEADER_LEN].copy_from_slice(&(reply.len() as u32).to_be_bytes());
                write_buf[HEADER_LEN..HEADER_LEN + reply.len()].copy_from_slice(reply.as_bytes());

                shared::write_full(fd, &write_buf)?;

                //

                let remaining = read_pos - consumed;

                read_buf.copy_within(consumed..read_pos, 0);
                read_pos = remaining;

                need_more = read_pos <= 0;
            }
            Err(err) => match err {
                shared::ParseRequestError::Incomplete => {
                    need_more = true;
                    println!("incomplete request");
                }
                shared::ParseRequestError::MessageTooLong(n) => {
                    println!("message too long (size={})", n);
                }
            },
        }
    }
}

fn main() -> Result<(), String> {
    // Create socket

    let fd = shared::create_socket()?;

    println!("created socket fd={}", fd);

    unsafe {
        let val = 1;

        setsockopt(
            fd,
            SOL_SOCKET,
            SO_REUSEADDR,
            &val as *const _ as *const libc::c_void,
            mem::size_of_val(&val) as libc::socklen_t,
        );
    };

    // Bind

    let addr = shared::make_addr([0, 0, 0, 0], 1234);

    let rv = unsafe {
        libc::bind(
            fd,
            &addr as *const _ as *const libc::sockaddr,
            mem::size_of_val(&addr) as libc::socklen_t,
        )
    };
    if rv != 0 {
        println!("unable to bind to 0.0.0.0:1234");
        std::process::exit(1);
    }

    // Listen

    let rv = unsafe { libc::listen(fd, SOMAXCONN) };
    if rv != 0 {
        println!("unable to bind to 0.0.0.0:1234");
        std::process::exit(1);
    }

    println!("listening on 0.0.0.0:1234");

    loop {
        let mut client_addr: libc::sockaddr_in = unsafe { mem::zeroed() };
        let mut client_addr_len: libc::socklen_t = unsafe { mem::zeroed() };

        let conn_fd = unsafe {
            libc::accept(
                fd,
                &mut client_addr as *mut _ as *mut libc::sockaddr,
                &mut client_addr_len,
            )
        };
        if conn_fd < 0 {
            println!("unable to accept connection");
            std::process::exit(1);
        }

        println!(
            "accepted connection from {}:{}, fd={}",
            client_addr.sin_addr.s_addr, client_addr.sin_port, conn_fd
        );

        // serve this connection indefinitely

        if let Err(err) = process_requests(conn_fd) {
            match err {
                ProcessOneRequestError::WriteError(_) | ProcessOneRequestError::ReadError(_) => {
                    println!(
                        "failed to process request, err: {}",
                        <ProcessOneRequestError as Into<String>>::into(err)
                    );
                }
                ProcessOneRequestError::EndOfStream => {
                    println!("end of stream, fd={}", conn_fd);
                }
            }
        }

        println!("closing fd={}", conn_fd);

        unsafe { libc::close(conn_fd) };
    }
}
