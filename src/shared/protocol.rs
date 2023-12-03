use onlyerror::Error;

use crate::{HEADER_LEN, MAX_MSG_LEN, RESPONSE_CODE_LEN, STRING_LEN};

const INTEGER_LEN: usize = 4;

#[derive(Error, Debug)]
pub enum Error {
    #[error("input too short ({0} bytes)")]
    InputTooShort(usize),
    #[error("message too long ({0} bytes)")]
    MessageTooLong(usize),
}

type Result<T> = std::result::Result<T, Error>;

pub fn parse_message(buf: &[u8]) -> Result<(usize, &[u8])> {
    if buf.len() < HEADER_LEN {
        return Err(Error::InputTooShort(buf.len()));
    }

    let message_len = {
        let header_data = &buf[0..HEADER_LEN];
        let len = u32::from_be_bytes(header_data.try_into().unwrap());

        len as usize
    };
    if message_len > MAX_MSG_LEN {
        return Err(Error::MessageTooLong(message_len));
    }

    if buf.len() < HEADER_LEN + message_len {
        return Err(Error::InputTooShort(buf.len()));
    }

    let body = &buf[HEADER_LEN..HEADER_LEN + message_len];
    let read = HEADER_LEN + body.len();

    Ok((read, body))
}

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn has_more(&self) -> bool {
        self.pos < self.buf.len()
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let buf = &self.buf[self.pos..];
        if buf.len() < INTEGER_LEN {
            return Err(Error::InputTooShort(buf.len()));
        }

        let n = {
            let data: [u8; 4] = buf[0..INTEGER_LEN].try_into().unwrap();
            u32::from_be_bytes(data)
        };

        self.pos += INTEGER_LEN;

        Ok(n)
    }

    pub fn read_string(&mut self) -> Result<&'a [u8]> {
        let n = self.read_u32()? as usize;

        let buf = &self.buf[self.pos..];
        if buf.len() < n {
            return Err(Error::InputTooShort(buf.len()));
        }

        let result = &buf[0..n];

        self.pos += result.len();

        Ok(result)
    }
}

pub struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        assert!(buf.len() >= HEADER_LEN + RESPONSE_CODE_LEN);

        Self {
            buf,
            pos: HEADER_LEN + RESPONSE_CODE_LEN,
        }
    }

    pub fn finish(&mut self) {
        let buf = &mut self.buf[0..HEADER_LEN];

        let written = self.pos - HEADER_LEN;

        buf.copy_from_slice(&(written as u32).to_be_bytes());
    }

    pub fn push_u32<T: Into<u32>>(&mut self, value: T) {
        let buf = &mut self.buf[HEADER_LEN..HEADER_LEN + RESPONSE_CODE_LEN];
        buf.copy_from_slice(&(value.into() as u32).to_be_bytes());
    }

    pub fn push_string<T: AsRef<[u8]>>(&mut self, value: T) {
        let bytes = value.as_ref();
        let buf = &mut self.buf[self.pos..];

        assert!(bytes.len() < buf.len());

        buf[0..STRING_LEN].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        buf[STRING_LEN..STRING_LEN + bytes.len()].copy_from_slice(bytes);

        self.pos += STRING_LEN + bytes.len()
    }

    pub fn written(&self) -> usize {
        self.pos
    }
}

pub fn buffer_size_needed(commands: &[Vec<&[u8]>]) -> usize {
    // Layout:
    //
    // * 4 bytes for the number of strings
    // per string
    // * 4 bytes for the length
    // * data bytes

    let size_for_all_strings = commands
        .iter()
        .fold(0, |acc, value| -> usize { acc + STRING_LEN + value.len() });

    4 + size_for_all_strings
}

#[cfg(test)]
mod tests {
    use crate::ResponseCode;

    use super::{parse_message, Writer};

    #[test]
    fn reader() {
        let data = b"\x00\x00\x00\x06foobar";

        let (read, request) = parse_message(data).unwrap();
        assert_eq!(10, read);
        assert_eq!(b"foobar", request);
    }

    #[test]
    fn writer() {
        let mut buf: [u8; 1024] = [0; 1024];

        let written = {
            let mut writer = Writer::new(&mut buf);
            writer.push_u32(ResponseCode::Nx);
            writer.push_string("foo");
            writer.push_string("bar");
            writer.finish();

            writer.written()
        };

        let written = &buf[0..written];
        assert_eq!(
            b"\x00\x00\x00\x12\x00\x00\x00\x02\x00\x00\x00\x03foo\x00\x00\x00\x03bar",
            written
        );
    }
}
