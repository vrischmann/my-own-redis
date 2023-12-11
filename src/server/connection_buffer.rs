use shared::protocol::BUF_LEN;

pub struct ConnectionBuffer {
    data: Vec<u8>,
    write_head: usize,
    read_head: usize,
}

impl ConnectionBuffer {
    pub fn new() -> Self {
        let mut data = Vec::with_capacity(BUF_LEN);
        data.resize(BUF_LEN, 0xaa);

        Self {
            data,
            write_head: 0,
            read_head: 0,
        }
    }

    pub fn reset(&mut self) {
        self.read_head = 0;
        self.write_head = 0;
    }

    pub fn is_empty(&self) -> bool {
        let remaining = self.read_head - self.write_head;
        remaining == 0
    }

    pub fn writable(&mut self) -> &mut [u8] {
        &mut self.data[self.write_head..]
    }

    pub fn readable(&self) -> &[u8] {
        &self.data[self.read_head..self.write_head]
    }

    pub fn update_write_head(&mut self, n: usize) {
        self.write_head += n;
        assert!(self.write_head < self.data.len());
    }

    pub fn update_read_head(&mut self, n: usize) {
        self.read_head += n;
        assert!(self.read_head <= self.write_head);
    }

    pub fn remove_processed(&mut self) {
        let remaining = self.write_head - self.read_head;
        if remaining <= 0 {
            return;
        }

        let next = self.read_head;

        println!(
            "move bytes from {:?} to the start of the read buf",
            next..next + remaining
        );

        self.data.copy_within(next..next + remaining, 0);
        self.read_head = 0;
    }
}

#[cfg(test)]
mod tests {
    use crate::ConnectionBuffer;

    #[test]
    fn connection_buffer() {
        let mut buffer = ConnectionBuffer::new();

        let written = {
            let buf = buffer.writable();
            buf[0..6].copy_from_slice("foobar".as_bytes());
            buf[6..12].copy_from_slice("foobar".as_bytes());

            12 as usize
        };
        buffer.update_write_head(written);

        assert_eq!(b"foobarfoobar", buffer.readable());
    }
}
