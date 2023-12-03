use onlyerror::Error;

use crate::{ResponseCode, HEADER_LEN, MAX_MSG_LEN, RESPONSE_CODE_LEN, STRING_LEN};

#[derive(Error, Debug)]
pub enum RequestParseError {
    #[error("not enough data")]
    NotEnoughData,
    #[error("message too long")]
    MessageTooLong,
}

pub fn parse_request(buf: &[u8]) -> Result<(usize, &[u8]), RequestParseError> {
    if buf.len() < HEADER_LEN {
        return Err(RequestParseError::NotEnoughData);
    }

    let message_len = {
        let header_data = &buf[0..HEADER_LEN];
        let len = u32::from_be_bytes(header_data.try_into().unwrap());

        len as usize
    };
    if message_len > MAX_MSG_LEN {
        return Err(RequestParseError::MessageTooLong);
    }

    if buf.len() < HEADER_LEN + message_len {
        return Err(RequestParseError::NotEnoughData);
    }

    let body = &buf[HEADER_LEN..HEADER_LEN + message_len];
    let read = HEADER_LEN + body.len();

    Ok((read, body))
}

pub struct ResponseWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> ResponseWriter<'a> {
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

    pub fn set_response_code(&mut self, code: ResponseCode) {
        let buf = &mut self.buf[HEADER_LEN..HEADER_LEN + RESPONSE_CODE_LEN];
        buf.copy_from_slice(&(code as u32).to_be_bytes());
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

#[cfg(test)]
mod tests {
    use crate::ResponseCode;

    use super::{parse_request, ResponseWriter};

    #[test]
    fn reader() {
        let data = b"\x00\x00\x00\x06foobar";

        let (read, request) = parse_request(data).unwrap();
        assert_eq!(10, read);
        assert_eq!(b"foobar", request);
    }

    #[test]
    fn response_writer() {
        let mut buf: [u8; 1024] = [0; 1024];

        let written = {
            let mut writer = ResponseWriter::new(&mut buf);
            writer.set_response_code(ResponseCode::Nx);
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
