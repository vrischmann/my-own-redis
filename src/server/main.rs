use libc::setsockopt;
use libc::AF_INET;
use libc::SOL_SOCKET;
use libc::SOMAXCONN;
use libc::SO_REUSEADDR;
use std::mem;

fn do_something(fd: i32) {
    // Read

    let mut read_buf: [u8; 64] = [0; 64];

    let n = unsafe {
        libc::read(
            fd,
            &mut read_buf as *mut _ as *mut libc::c_void,
            read_buf.len() - 1,
        )
    };
    if n < 0 {
        println!("unable to read from fd");
        std::process::exit(1);
    }

    let data: &[u8] = &read_buf[0..n as usize];

    println!("client says \"{}\"", String::from_utf8_lossy(data));

    // Write

    unsafe {
        let response = "world";
        libc::write(
            fd,
            response as *const _ as *const libc::c_void,
            response.len(),
        );
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

    let addr = libc::sockaddr_in {
        sin_family: AF_INET as libc::sa_family_t,
        sin_port: (1234 as u16).to_be(),
        sin_addr: libc::in_addr {
            s_addr: (0 as u32).to_be(),
        },
        sin_zero: [0; 8],
    };

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

        do_something(conn_fd);

        unsafe { libc::close(conn_fd) };
    }
}
