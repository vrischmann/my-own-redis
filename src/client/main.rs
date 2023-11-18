use std::mem;

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

    // Write

    shared::write(fd, "hello".as_bytes())?;

    // Read

    let mut read_buf: [u8; 64] = [0; 64];
    let data = shared::read(fd, &mut read_buf)?;

    println!("server says \"{}\"", String::from_utf8_lossy(data));

    unsafe {
        libc::close(fd);
    }

    Ok(())
}
