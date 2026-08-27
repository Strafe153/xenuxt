mod http;
mod reader;
mod writer;

use std::{env, error::Error, net::TcpListener, process::exit};

use crate::{http::*, reader::Reader, writer::Writer};

fn main() -> Result<(), Box<dyn Error>> {
    let port = get_port();

    let mut store = HttpHandlerStore::new();
    register_handlers(&mut store).expect("Failed to register handlers");

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(_) => {
            println!("Failed to bind to TCP port {}", port);
            exit(1);
        }
    };

    println!("Listening on port {}...", port);

    for stream_result in listener.incoming() {
        match stream_result {
            Ok(stream) => {
                match stream.try_clone() {
                    Ok(s) => {
                        let mut reader = Reader::new(stream);
                        let mut writer = Writer::new(s);

                        // instead of using ? handle errors here, since right now it's main (which should be refactored)
                        // but it's the last chance to gracefully handle them
                        match reader.read_request_line() {
                            Ok(request_line) => {
                                let request_line = HttpRequestLine::try_from(request_line)?;
                                match read_request_headers(&mut reader) {
                                    Ok(headers) => {
                                        let content_length = headers.iter().find(|&h| {
                                            h.name.as_str().eq_ignore_ascii_case("Content-Length")
                                        });

                                        handle_content_length(
                                            content_length,
                                            request_line,
                                            &mut reader,
                                            &mut writer,
                                            &store,
                                        );
                                    }
                                    Err(ReadRequestError::IoError(e)) => {
                                        write(&mut writer, HttpResponse::error_bad_request(e.to_string()));
                                    }
                                    Err(ReadRequestError::HttpParseError(e)) => {
                                        write(&mut writer, HttpResponse::error_bad_request(e.to_string()));
                                    }
                                }
                            }
                            Err(e) => println!("{}", e),
                        }
                    }
                    Err(e) => println!("{}", e),
                }
            }
            Err(e) => println!("{}", e),
        }
    }

    Ok(())
}

fn get_port() -> u16 {
    match env::args().nth(1) {
        Some(a) => match a.parse::<u16>() {
            Ok(p) => p,
            Err(_) => {
                println!("Invalid port");
                exit(1);
            }
        },
        None => 1717,
    }
}

// still needs further refactoring
fn handle_content_length(
    content_length: Option<&HttpHeader>,
    request_line: HttpRequestLine,
    reader: &mut Reader,
    writer: &mut Writer,
    store: &HttpHandlerStore,
) {
    let Some(header) = content_length else {
        if request_line.requires_body() {
            write(writer, HttpResponse::length_required());
        }

        return;
    };

    let size = match header.value.parse::<usize>() {
        Ok(size) => size,
        Err(_) => {
            write(writer, HttpResponse::error_bad_request("\"Content-Length\" value MUST be numeric"));
            return;
        }
    };

    let body = match reader.read_body(size) {
        Ok(body) => body,
        Err(_) => {
            write(
                writer,
                HttpResponse::bad_request(None, Some(b"Failed to read request body".to_vec())),
            );
            return;
        }
    };

    match store.get(request_line) {
        HttpHandler::Found(handler) => {
            let body = HttpBody::new(body).value();
            write(writer, handler(Some(body)));
        }
        HttpHandler::MethodNotAllowed(m) => write(writer, HttpResponse::method_not_allowed(m)),
        HttpHandler::NotFound => write(writer, HttpResponse::not_found(None, None)),
    }
}

fn write(writer: &mut Writer, response: HttpResponse) {
    if let Err(_) = writer.write(response) {
        println!("Failed to write the response")
    }
}

enum ReadRequestError {
    IoError(std::io::Error),
    HttpParseError(HttpParseError),
}

impl From<std::io::Error> for ReadRequestError {
    fn from(value: std::io::Error) -> Self {
        Self::IoError(value)
    }
}

impl From<HttpParseError> for ReadRequestError {
    fn from(value: HttpParseError) -> Self {
        Self::HttpParseError(value)
    }
}

fn read_request_headers(reader: &mut Reader) -> Result<Vec<HttpHeader>, ReadRequestError> {
    let mut headers = Vec::new();

    loop {
        let header = reader.read_header()?;

        if header == CRLF {
            break;
        }

        match HttpHeader::try_from(header) {
            Ok(h) => headers.push(h),
            Err(e) => return Err(e.into()),
        }
    }

    Ok(headers)
}

// maybe use serde_json for serialization
fn register_handlers(store: &mut HttpHandlerStore) -> Result<(), std::io::Error> {
    store.register("/", HttpMethod::GET, |_| {
        HttpResponse::ok(
            None,
            Some("{\r\n\t\"response\": \"test\"\r\n}\r\n".as_bytes().to_vec()),
        )
    })?;

    store.register("/bad", HttpMethod::POST, |_| {
        HttpResponse::bad_request(
            None,
            Some(
                "{\r\n\t\"response\": \"bad request\"\r\n}\r\n"
                    .as_bytes()
                    .to_vec(),
            ),
        )
    })?;

    Ok(())
}
