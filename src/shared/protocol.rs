use onlyerror::Error;

use crate::{HEADER_LEN, MAX_MSG_LEN};

#[derive(Error, Debug)]
pub enum RequestParseError {
    #[error("not enough data")]
    NotEnoughData,
    #[error("message too long")]
    MessageTooLong,
}

pub fn parse_request(buf: &[u8]) -> Result<&[u8], RequestParseError> {
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

    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::parse_request;

    #[test]
    fn reader() {
        let data = b"\x00\x00\x00\x06foobar";

        let request = parse_request(data).unwrap();
        assert_eq!(b"foobar", request);
    }
}
