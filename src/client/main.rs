use libc::{AF_INET, INADDR_LOOPBACK};
use std::mem;

fn main() -> Result<(), String> {
    // Create socket

    let fd = shared::create_socket()?;

    println!("created socket fd={}", fd);

    // Connect

    let addr = libc::sockaddr_in {
        sin_family: AF_INET as libc::sa_family_t,
        sin_port: (1234 as u16).to_be(),
        sin_addr: libc::in_addr {
            s_addr: INADDR_LOOPBACK.to_be(),
        },
        sin_zero: [0; 8],
    };

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

    // Write

    let n = unsafe {
        let data = "hello";
        libc::write(fd, data as *const _ as *const libc::c_void, data.len())
    };
    if n < 0 {
        println!("unable to write to fd");
        std::process::exit(1);
    }

    // READ

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

    println!("server says \"{}\"", String::from_utf8_lossy(data));

    unsafe {
        libc::close(fd);
    }

    Ok(())
}
