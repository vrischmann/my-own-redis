use onlyerror::Error;

const HEADER_LEN: usize = 4;
pub const MAX_MSG_LEN: usize = 4096;
pub const BUF_LEN: usize = HEADER_LEN + MAX_MSG_LEN;
const INTEGER_LEN: usize = 4;
const STRING_LEN: usize = 4;

#[derive(Error, Debug)]
pub enum Error {
    #[error("input too short ({0} bytes)")]
    InputTooShort(usize),
    #[error("message too long ({0} bytes)")]
    MessageTooLong(usize),
}

type Result<T> = std::result::Result<T, Error>;

pub fn parse_message(buf: &[u8]) -> Result<(usize, &[u8])> {
    println!("buf: {:?}", buf);

    if buf.len() < HEADER_LEN {
        return Err(Error::InputTooShort(buf.len()));
    }

    let mut reader = Reader::new(buf);

    let message_len = reader.read_u32()? as usize;
    if message_len > MAX_MSG_LEN {
        return Err(Error::MessageTooLong(message_len));
    }

    let remaining = reader.remaining();
    if remaining.len() < message_len {
        return Err(Error::InputTooShort(remaining.len()));
    }

    Ok((buf.len(), remaining))
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

    pub fn remaining(self) -> &'a [u8] {
        &self.buf[self.pos..]
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

/// Wraps a buffer and provides methods to serialize data to the buffer.
///
/// # Examples
///
/// ```
/// use shared::protocol::{BUF_LEN, Writer};
/// let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];
///
/// let written = {
///     let mut writer = Writer::new(&mut buf);
///     writer.push_u32(2 as u32);
///     writer.push_string("hello");
///     writer.push_string("hallo");
///     writer.finish();
///     writer.written()
/// };
///
/// assert_eq!(
///     &[
///         0x00, 0x00, 0x00, 0x16, // message length in bytes
///         0x00, 0x00, 0x00, 0x02, // number of strings
///         0x00, 0x00, 0x00, 0x05, // first string length in bytes
///         b'h', b'e', b'l', b'l', b'o', // first string data
///         0x00, 0x00, 0x00, 0x05, // second string length in bytes
///         b'h', b'a', b'l', b'l', b'o', // second string data
///     ],
///     &buf[0..written],
/// );
/// ```
pub struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    /// Creates a new `Writer` wrapping the provided slice.
    ///
    /// # Examples
    /// ```no_run
    /// use shared::protocol::{BUF_LEN, Writer};
    /// let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];
    ///
    /// let mut writer = Writer::new(&mut buf);
    pub fn new(buf: &'a mut [u8]) -> Self {
        assert!(buf.len() == BUF_LEN);

        Self {
            buf,
            pos: HEADER_LEN, // offset 4 bytes to keep space for the length when calling finish()
        }
    }

    /// Finish writing. This will write the message length in the first 4 bytes of the buffer.
    /// Call this when you're done writing your message.
    ///
    /// # Examples
    /// ```
    /// # use shared::protocol::{BUF_LEN, Writer};
    /// # let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];
    ///
    /// let mut writer = Writer::new(&mut buf);
    /// writer.push_u32(2 as u32);
    /// writer.finish();
    ///
    /// assert_eq!(
    ///     &[
    ///         0x00, 0x00, 0x00, 0x04, // message length in bytes
    ///         0x00, 0x00, 0x00, 0x02, // u32
    ///     ],
    ///     &buf[0..8],
    /// );
    /// ```
    pub fn finish(&mut self) {
        let buf = &mut self.buf[0..HEADER_LEN];

        let written = self.pos - HEADER_LEN;

        buf.copy_from_slice(&(written as u32).to_be_bytes());
    }

    /// Write a u32 to the buffer, encoded as 4 using big-endian encoding.
    ///
    /// # Examples
    /// ```
    /// # use shared::protocol::{BUF_LEN, Writer};
    /// # let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];
    ///
    /// let mut writer = Writer::new(&mut buf);
    /// writer.push_u32(20 as u32);
    /// writer.finish();
    ///
    /// assert_eq!(
    ///     &[
    ///         0x00, 0x00, 0x00, 0x04, // message length in bytes
    ///         0x00, 0x00, 0x00, 0x14, // u32
    ///     ],
    ///     &buf[0..8],
    /// );
    /// ```
    pub fn push_u32<T: Into<u32>>(&mut self, value: T) {
        let buf = &mut self.buf[self.pos..self.pos + INTEGER_LEN];

        buf.copy_from_slice(&(value.into() as u32).to_be_bytes());

        self.pos += INTEGER_LEN
    }

    /// Write a string to the buffer.
    /// A string is a made of two parts:
    /// * a u32 representing its length
    /// * `length` bytes of data
    ///
    /// # Examples
    /// ```
    /// # use shared::protocol::{BUF_LEN, Writer};
    /// # let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];
    ///
    /// let mut writer = Writer::new(&mut buf);
    /// writer.push_string("foobar");
    /// writer.finish();
    ///
    /// assert_eq!(
    ///     &[
    ///         0x00, 0x00, 0x00, 0x0A, // message length in bytes
    ///         0x00, 0x00, 0x00, 0x06, // string length
    ///         b'f', b'o', b'o', b'b', b'a', b'r', // string data
    ///     ],
    ///     &buf[0..14],
    /// );
    /// ```
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
    use crate::{protocol::BUF_LEN, ResponseCode};

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
        let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];

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
