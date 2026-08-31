use std::{
    error::Error,
    fmt::Display,
    io::{BufRead, BufReader, ErrorKind, Read},
    net::TcpStream,
    time::Duration,
};

const MAX_CONTENT_LENGTH: usize = 1024 * 1024;
const MAX_LINE_LENGTH: u64 = 8192;

#[derive(Debug)]
pub enum ReadError {
    PayloadTooLarge,
    UriTooLong,
    HeaderTooLong,
    IoError(std::io::Error),
}

impl Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PayloadTooLarge => write!(f, "Payload too large"),
            Self::UriTooLong => write!(f, "URI length CANNOT exceed {}", MAX_LINE_LENGTH),
            Self::HeaderTooLong => write!(f, "Header length CANNOT exceed {}", MAX_LINE_LENGTH),
            Self::IoError(e) => write!(f, "{}", e),
        }
    }
}

impl From<std::io::Error> for ReadError {
    fn from(value: std::io::Error) -> Self {
        ReadError::IoError(value)
    }
}

impl Error for ReadError {}

type Result<T> = core::result::Result<T, ReadError>;

pub struct Reader(BufReader<TcpStream>);

impl Reader {
    pub fn new(stream: TcpStream) -> Self {
        _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        Self(BufReader::new(stream))
    }

    pub fn read_request_line(&mut self) -> Result<String> {
        self.read_line(ReadError::UriTooLong)
    }

    pub fn read_header(&mut self) -> Result<String> {
        self.read_line(ReadError::HeaderTooLong)
    }

    pub fn read_body(&mut self, length: usize) -> Result<Vec<u8>> {
        if length > MAX_CONTENT_LENGTH {
            return Err(ReadError::PayloadTooLarge);
        }

        let mut buffer = vec![0; length];
        self.0.read_exact(&mut buffer)?;

        Ok(buffer)
    }

    fn read_line(&mut self, error: ReadError) -> Result<String> {
        let mut line = String::new();
        let bytes_read = self.0.by_ref().take(MAX_LINE_LENGTH).read_line(&mut line)?;

        if bytes_read == 0 {
            return Err(std::io::Error::new(ErrorKind::UnexpectedEof, "Unexpected EOF.").into());
        }

        if !line.ends_with("\r\n") {
            return Err(error);
        }

        Ok(line)
    }
}
