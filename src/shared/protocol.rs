use onlyerror::Error;
use std::{fmt, mem};

const HEADER_LEN: usize = 4;
pub const MAX_MSG_LEN: usize = 4096;
pub const BUF_LEN: usize = HEADER_LEN + MAX_MSG_LEN;
const RESPONSE_CODE_LEN: usize = 4;
const DATA_TYPE_LEN: usize = 1;
const INTEGER_LEN: usize = 8;
const STRING_LEN: usize = 4;

#[derive(Error, Debug)]
pub enum Error {
    #[error("input too short ({0} bytes)")]
    InputTooShort(usize),
    #[error("message too long ({0} bytes)")]
    MessageTooLong(usize),
    #[error("invalid data type {0}")]
    InvalidDataType(u8),
    #[error("invalid response code {0}")]
    InvalidResponseCode(u32),
    #[error("incoherent data type, want {want} but got {got}")]
    IncoherentDataType { got: DataType, want: DataType },
}

type Result<T> = std::result::Result<T, Error>;

pub fn parse_message(buf: &[u8]) -> Result<(usize, &[u8])> {
    const N: usize = mem::size_of::<u32>();

    // 1. Get the message length

    if buf.len() < HEADER_LEN {
        return Err(Error::InputTooShort(buf.len()));
    }

    let length = {
        let data: [u8; N] = buf[0..N].try_into().unwrap();
        u32::from_be_bytes(data) as usize
    };

    if length > MAX_MSG_LEN {
        return Err(Error::MessageTooLong(length));
    }

    // 2. Compute the results

    let read = N + length;
    let message = &buf[N..N + length];

    Ok((read, message))
}

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

trait FromBytes<const N: usize> {
    fn from_be_bytes(arr: [u8; N]) -> Self;
}

impl FromBytes<{ mem::size_of::<u64>() }> for u64 {
    fn from_be_bytes(arr: [u8; mem::size_of::<u64>()]) -> Self {
        Self::from_be_bytes(arr)
    }
}
impl FromBytes<{ mem::size_of::<u32>() }> for u32 {
    fn from_be_bytes(arr: [u8; mem::size_of::<u32>()]) -> Self {
        Self::from_be_bytes(arr)
    }
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

    pub fn clone_remaining(&self) -> Vec<u8> {
        let mut res = Vec::new();

        res.extend_from_slice(&self.buf[self.pos..]);

        res
    }

    fn read_int_<T: FromBytes<N>, const N: usize>(&mut self) -> Result<T> {
        let buf = &self.buf[self.pos..];

        if buf.len() < N {
            return Err(Error::InputTooShort(buf.len()));
        }

        let result = {
            let data: [u8; N] = buf[0..N].try_into().unwrap();
            T::from_be_bytes(data)
        };

        self.pos += N;

        Ok(result)
    }

    pub fn read_data_type(&mut self) -> Result<DataType> {
        eprintln!(
            "\x1b[34m==> start/read_data_type/body: {:?}\x1b[0m",
            self.clone_remaining()
        );

        if self.pos >= self.buf.len() {
            return Err(Error::InputTooShort(self.buf.len()));
        }

        let result = match self.buf[self.pos] {
            0 => DataType::Nil,
            1 => DataType::Err,
            2 => DataType::Str,
            3 => DataType::Int,
            4 => DataType::Arr,
            n => return Err(Error::InvalidDataType(n)),
        };

        self.pos += 1;

        eprintln!(
            "\x1b[34m==> end/read_data_type/body: {:?}\x1b[0m",
            self.clone_remaining()
        );

        Ok(result)
    }

    pub fn read_int(&mut self) -> Result<u64> {
        let data_type = self.read_data_type()?;
        if data_type != DataType::Int {
            return Err(Error::IncoherentDataType {
                want: DataType::Str,
                got: data_type,
            });
        }

        const N: usize = mem::size_of::<u64>();
        self.read_int_::<u64, N>()
    }

    pub fn read_string(&mut self) -> Result<&'a [u8]> {
        eprintln!(
            "\x1b[34m==> start/read_string/body: {:?}\x1b[0m",
            self.clone_remaining()
        );

        let data_type = self.read_data_type()?;
        if data_type != DataType::Str {
            return Err(Error::IncoherentDataType {
                want: DataType::Str,
                got: data_type,
            });
        }

        let length: u32 = self.read_int_()?;

        let result = &self.buf[self.pos..self.pos + length as usize];
        self.pos += result.len();

        eprintln!(
            "\x1b[34m==> end/read_string/body: {:?}\x1b[0m",
            self.clone_remaining()
        );

        Ok(result)
    }

    pub fn read_err(&mut self) -> Result<(u32, &[u8])> {
        eprintln!(
            "\x1b[34m==> start/read_err/body: {:?}\x1b[0m",
            self.clone_remaining()
        );

        const N: usize = mem::size_of::<u32>();

        let response_code = self.read_int_::<u32, N>()?;
        let length: u32 = self.read_int_()?;

        let buf = &self.buf[self.pos..];
        let result = &buf[0..length as usize];
        self.pos += result.len();

        eprintln!(
            "\x1b[34m==> end/read_err/body: {:?}\x1b[0m",
            self.clone_remaining()
        );

        Ok((response_code, result))
    }

    pub fn read_data_type_err(&mut self) -> Result<(u32, &[u8])> {
        eprintln!(
            "\x1b[34m==> start/read_data_type_err/body: {:?}\x1b[0m",
            self.clone_remaining()
        );

        let data_type = self.read_data_type()?;
        if data_type != DataType::Err {
            return Err(Error::IncoherentDataType {
                want: DataType::Str,
                got: data_type,
            });
        }

        eprintln!(
            "\x1b[34m==> end/read_data_type_err/body: {:?}\x1b[0m",
            self.clone_remaining()
        );

        self.read_err()
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
///     writer.push_int(2);
///     writer.push_string("hello");
///     writer.push_string("hallo");
///     writer.finish();
///     writer.written()
/// };
///
/// assert_eq!(
///     &[
///         0x00, 0x00, 0x00, 0x1C,        // message length in bytes
///         0x00, 0x00, 0x00, 0x00,
///         0x00, 0x00, 0x00, 0x02,        // number of strings
///         0x02,                          // string data type
///         0x00, 0x00, 0x00, 0x05,        // first string length in bytes
///         b'h', b'e', b'l', b'l', b'o',  // first string data
///         0x02,                          // string data type
///         0x00, 0x00, 0x00, 0x05,        // second string length in bytes
///         b'h', b'a', b'l', b'l', b'o',  // second string data
///     ],
///     &buf[0..written],
/// );
/// ```
pub struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DataType {
    Nil = 0,
    Err = 1,
    Str = 2,
    Int = 3,
    Arr = 4,
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::Nil => write!(f, "nil"),
            DataType::Err => write!(f, "err"),
            DataType::Str => write!(f, "str"),
            DataType::Int => write!(f, "int"),
            DataType::Arr => write!(f, "arr"),
        }
    }
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
        assert_eq!(buf.len(), BUF_LEN);

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
    /// writer.push_int(8);
    /// writer.finish();
    ///
    /// assert_eq!(
    ///     &[
    ///         0x00, 0x00, 0x00, 0x08, // message length in bytes
    ///         0x00, 0x00, 0x00, 0x00,
    ///         0x00, 0x00, 0x00, 0x08, // u64
    ///     ],
    ///     &buf[0..12],
    /// );
    /// ```
    pub fn finish(&mut self) {
        let buf = &mut self.buf[0..HEADER_LEN];

        let written = self.pos - HEADER_LEN;

        buf.copy_from_slice(&(written as u32).to_be_bytes());
    }

    /// Write a nil to the buffer.
    /// A nil is made of a single part:
    /// * a u8 representing its data type (the value <b>0</b>)
    ///
    /// # Examples
    /// ```
    /// # use shared::protocol::{BUF_LEN, Writer};
    /// # let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];
    ///
    /// let mut writer = Writer::new(&mut buf);
    /// writer.push_nil();
    /// writer.finish();
    ///
    /// assert_eq!(
    ///     &[
    ///         0x00, 0x00, 0x00, 0x01, // message length in bytes
    ///         0x00,                   // Nil data type
    ///     ],
    ///     &buf[0..5],
    /// );
    /// ```
    pub fn push_nil(&mut self) {
        self.buf[self.pos] = DataType::Nil as u8;
        self.pos += 1
    }

    /// Write a u32 to the buffer, encoded as 4 using big-endian encoding.
    ///
    /// # Examples
    /// ```
    /// # use shared::protocol::{BUF_LEN, Writer};
    /// # let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];
    ///
    /// let mut writer = Writer::new(&mut buf);
    /// writer.push_int(20);
    /// writer.finish();
    ///
    /// assert_eq!(
    ///     &[
    ///         0x00, 0x00, 0x00, 0x08, // message length in bytes
    ///         0x00, 0x00, 0x00, 0x00,
    ///         0x00, 0x00, 0x00, 0x14, // u64
    ///     ],
    ///     &buf[0..12],
    /// );
    /// ```
    pub fn push_int(&mut self, value: usize) {
        let buf = &mut self.buf[self.pos..self.pos + DATA_TYPE_LEN + INTEGER_LEN];

        buf[0] = DataType::Int as u8;
        buf[1..].copy_from_slice(&(value as u64).to_be_bytes());

        self.pos += DATA_TYPE_LEN + INTEGER_LEN
    }

    /// Write a string to the buffer.
    /// A string is made of three parts:
    /// * a u8 representing its data type (the value <b>2</b>)
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
    ///         0x00, 0x00, 0x00, 0x0B,             // message length in bytes
    ///         0x02,                               // string data type
    ///         0x00, 0x00, 0x00, 0x06,             // string length
    ///         b'f', b'o', b'o', b'b', b'a', b'r', // string data
    ///     ],
    ///     &buf[0..15],
    /// );
    /// ```
    pub fn push_string<T: AsRef<[u8]>>(&mut self, value: T) {
        let bytes = value.as_ref();
        let buf = &mut self.buf[self.pos..];

        assert!(buf.len() > bytes.len() + DATA_TYPE_LEN + STRING_LEN);

        buf[0] = DataType::Str as u8;
        buf[1..5].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        buf[5..5 + bytes.len()].copy_from_slice(bytes);

        self.pos += DATA_TYPE_LEN + STRING_LEN + bytes.len()
    }

    pub fn push_err<C: Into<u32>, T: AsRef<[u8]>>(&mut self, code: C, message: T) {
        let bytes = message.as_ref();
        let buf = &mut self.buf[self.pos..];

        assert!(buf.len() >= DATA_TYPE_LEN + RESPONSE_CODE_LEN + STRING_LEN + bytes.len());

        buf[0] = DataType::Err as u8;
        buf[1..5].copy_from_slice(&code.into().to_be_bytes());
        buf[5..9].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        buf[9..9 + bytes.len()].copy_from_slice(bytes);

        self.pos += DATA_TYPE_LEN + RESPONSE_CODE_LEN + STRING_LEN + bytes.len();
    }

    /// Return the number of bytes written into the buffer
    /// Note that there's always 4 bytes written for the message length, even if you don't push anything.
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
    /// assert_eq!(15, writer.written());
    /// ```
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
    fn writer_write_response() {
        let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];

        let written = {
            let mut writer = Writer::new(&mut buf);
            writer.push_err(ResponseCode::TooBig as u32, "foo");
            writer.push_string("bar");
            writer.finish();

            writer.written()
        };

        let written = &buf[0..written];
        assert_eq!(
            b"\x00\x00\x00\x14\x01\x00\x00\x00\x65\x00\x00\x00\x03foo\x02\x00\x00\x00\x03bar",
            written
        );
    }

    #[test]
    fn writer_push_nil() {
        let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];

        let written = {
            let mut writer = Writer::new(&mut buf);
            writer.push_nil();
            writer.finish();
            writer.written()
        };

        let written = &buf[0..written];
        assert_eq!(b"\x00\x00\x00\x01\x00", written);
    }

    #[test]
    fn writer_push_string() {
        let mut buf: [u8; BUF_LEN] = [0; BUF_LEN];

        let written = {
            let mut writer = Writer::new(&mut buf);
            writer.push_string("foo");
            writer.finish();
            writer.written()
        };

        let written = &buf[0..written];
        assert_eq!(b"\x00\x00\x00\x08\x02\x00\x00\x00\x03foo", written);
    }
}
