use libc::setsockopt;
use libc::SOL_SOCKET;
use libc::SOMAXCONN;
use libc::SO_REUSEADDR;
use std::mem;

enum DoSomethingError {
    ReadError(shared::ReadError),
    WriteError(shared::WriteError),
}

impl From<DoSomethingError> for String {
    fn from(err: DoSomethingError) -> String {
        match err {
            DoSomethingError::ReadError(err) => err.into(),
            DoSomethingError::WriteError(err) => err.into(),
        }
    }
}

impl From<shared::ReadError> for DoSomethingError {
    fn from(err: shared::ReadError) -> DoSomethingError {
        DoSomethingError::ReadError(err)
    }
}

impl From<shared::WriteError> for DoSomethingError {
    fn from(err: shared::WriteError) -> DoSomethingError {
        DoSomethingError::WriteError(err)
    }
}

fn do_something(fd: i32) -> Result<(), DoSomethingError> {
    // Read

    let mut read_buf: [u8; 64] = [0; 64];
    let data = shared::read(fd, &mut read_buf)?;

    println!("client says \"{}\"", String::from_utf8_lossy(data));

    // Write

    shared::write(fd, "world".as_bytes())?;

    Ok(())
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

        do_something(conn_fd)?;

        unsafe { libc::close(conn_fd) };
    }
}
