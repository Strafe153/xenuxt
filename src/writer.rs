use std::{
    io::{BufWriter, Error, Write},
    net::TcpStream,
};

use crate::http::HttpResponse;

pub struct Writer(BufWriter<TcpStream>);

impl Writer {
    pub fn new(stream: TcpStream) -> Self {
        Self(BufWriter::new(stream))
    }

    pub fn write(&mut self, response: HttpResponse) -> Result<(), Error> {
        let mut data: Vec<u8> = response.into();

        self.0.write_all(&mut data)?;
        self.0.flush()?;

        Ok(())
    }
}
