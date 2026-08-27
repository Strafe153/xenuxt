use std::{
    io::{BufRead, BufReader, Error, ErrorKind, Read},
    net::TcpStream,
    time::Duration,
};

const MAX_CONTENT_LENGTH: usize = 1024 * 1024;
const MAX_LINE_LENGTH: u64 = 8192;

type Result<T> = core::result::Result<T, std::io::Error>;

pub struct Reader(BufReader<TcpStream>);

impl Reader {
    pub fn new(stream: TcpStream) -> Self {
        _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        Self(BufReader::new(stream))
    }

    pub fn read_request_line(&mut self) -> Result<String> {
        self.read_line()
    }

    pub fn read_header(&mut self) -> Result<String> {
        self.read_line()
    }

    pub fn read_body(&mut self, length: usize) -> Result<Vec<u8>> {
        if length > MAX_CONTENT_LENGTH {
            return Err(Error::new(ErrorKind::InvalidData, "Payload too large"));
        }

        let mut buffer = vec![0; length];
        self.0.read_exact(&mut buffer)?;

        Ok(buffer)
    }

    fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        let bytes_read = self.0.by_ref().take(MAX_LINE_LENGTH).read_line(&mut line)?;

        if bytes_read == 0 {
            return Err(Error::new(ErrorKind::UnexpectedEof, "Unexpected EOF."));
        }

        if !line.ends_with("\r\n") {
            return Err(Error::new(ErrorKind::InvalidData, "Incomplete HTTP line."));
        }

        Ok(line)
    }
}
