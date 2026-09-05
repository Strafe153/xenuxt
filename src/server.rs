use std::{error::Error, io::ErrorKind, net::TcpListener};

use crate::{
    http::*,
    reader::{ReadError, Reader},
    writer::Writer,
};

pub fn run(store: HttpHandlerStore) {
    run_on_port(3017, store)
}

pub fn run_on_port(port: u16, store: HttpHandlerStore) {
    let listener = bind_listener(port);

    println!("Listening on port {}...", port);

    listen(listener, store, port);
}

fn bind_listener(port: u16) -> TcpListener {
    TcpListener::bind((LOCALHOST_IP_V4, port))
        .unwrap_or_else(|_| panic!("Failed to bind to TCP port {}", port))
}

fn read_request_headers(reader: &mut Reader) -> Result<Vec<HttpHeader>, Box<dyn Error>> {
    let mut headers = Vec::new();

    loop {
        let header = reader.read_header()?;

        if header == CRLF {
            return Ok(headers);
        }

        headers.push(HttpHeader::try_from(header)?);
    }
}

fn write(writer: &mut Writer, response: HttpResponse) {
    if let Err(_) = writer.write(response) {
        println!("Failed to write the response")
    }
}

enum IterationResult<T> {
    Value(T),
    Continue,
    Break,
}

fn get_request_line(reader: &mut Reader, writer: &mut Writer) -> IterationResult<HttpRequestLine> {
    let request_line = match reader.read_request_line() {
        Ok(l) => l,
        Err(e @ ReadError::UriTooLong) => {
            write(writer, HttpResponse::uri_too_long_err(e.to_string()));
            return IterationResult::Continue;
        }
        Err(ReadError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => {
            return IterationResult::Break;
        }
        Err(_) => {
            write(
                writer,
                HttpResponse::bad_request_err("Failed to read the request line"),
            );

            return IterationResult::Break;
        }
    };

    let request_line = match HttpRequestLine::try_from(request_line) {
        Ok(line) => line,
        Err(e) => {
            write(writer, HttpResponse::bad_request_err(e.to_string()));
            return IterationResult::Continue;
        }
    };

    IterationResult::Value(request_line)
}

fn get_headers(reader: &mut Reader, writer: &mut Writer) -> IterationResult<Vec<HttpHeader>> {
    let headers = match read_request_headers(reader) {
        Ok(h) => h,
        Err(e) => {
            if let Some(ReadError::IoError(e)) = e.downcast_ref::<ReadError>()
                && e.kind() == ErrorKind::UnexpectedEof
            {
                return IterationResult::Break;
            }

            if let Some(e) = e.downcast_ref::<ReadError>() {
                write(writer, HttpResponse::header_too_long_err(e.to_string()));

                return IterationResult::Continue;
            }

            write(
                writer,
                HttpResponse::bad_request_err("Failed to read request headers"),
            );

            return IterationResult::Break;
        }
    };

    IterationResult::Value(headers)
}

fn listen(listener: TcpListener, store: HttpHandlerStore, port: u16) {
    // TODO: Potentially, add support for multiple clients using thread::spawn
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            println!("Failed to accept a connection");
            continue;
        };

        let Ok(stream_copy) = stream.try_clone() else {
            println!("Failed to clone the connection");
            continue;
        };

        let mut reader = Reader::new(stream);
        let mut writer = Writer::new(stream_copy);

        loop {
            let request_line = match get_request_line(&mut reader, &mut writer) {
                IterationResult::Value(l) => l,
                IterationResult::Continue => continue,
                IterationResult::Break => break,
            };

            let headers = match get_headers(&mut reader, &mut writer) {
                IterationResult::Value(l) => l,
                IterationResult::Continue => continue,
                IterationResult::Break => break,
            };

            if let Err(e) = validate_host(&headers, port) {
                write(&mut writer, HttpResponse::bad_request_err(e.to_string()));
                continue;
            }

            if let Err(e) = validate_unsupported(&headers) {
                write(&mut writer, HttpResponse::bad_request_err(e.to_string()));
                continue;
            }

            if request_line.expects_body() {
                handle_with_body(request_line, headers, &mut reader, &mut writer, &store);
                continue;
            }

            handle_without_body(request_line, headers, &mut writer, &store);
        }
    }
}

fn handle(
    request_line: HttpRequestLine,
    headers: Vec<HttpHeader>,
    writer: &mut Writer,
    store: &HttpHandlerStore,
    body: Option<Vec<u8>>,
) {
    match store.get(&request_line) {
        HttpHandler::Found(handler) => {
            let payload = RequestPayload::new(Some(headers), body, request_line.query_string);
            write(writer, handler(payload));
        }
        HttpHandler::MethodNotAllowed(method) => {
            write(writer, HttpResponse::method_not_allowed(method))
        }
        HttpHandler::NotFound => write(writer, HttpResponse::not_found(None, None)),
    }
}

fn handle_without_body(
    request_line: HttpRequestLine,
    headers: Vec<HttpHeader>,
    writer: &mut Writer,
    store: &HttpHandlerStore,
) {
    let content_length = match validate_content_length(&headers) {
        Ok(value) => value,
        Err(e) => {
            write(writer, HttpResponse::bad_request_err(e.to_string()));
            return;
        }
    };

    if content_length.is_some() {
        write(
            writer,
            HttpResponse::bad_request_err(format!(
                "'{}' is NOT allowed for this HTTP method",
                CONTENT_LENGTH
            )),
        );
        return;
    }

    handle(request_line, headers, writer, store, None);
}

fn handle_with_body(
    request_line: HttpRequestLine,
    headers: Vec<HttpHeader>,
    reader: &mut Reader,
    writer: &mut Writer,
    store: &HttpHandlerStore,
) {
    let content_length = match validate_content_length(&headers) {
        Ok(Some(value)) => value,
        Ok(None) => {
            write(writer, HttpResponse::length_required());
            return;
        }
        Err(e) => {
            write(writer, HttpResponse::bad_request_err(e.to_string()));
            return;
        }
    };

    let Ok(size) = content_length.value.parse::<usize>() else {
        write(
            writer,
            HttpResponse::bad_request_err(format!("'{}' value MUST be numeric", CONTENT_LENGTH)),
        );
        return;
    };

    let body = match reader.read_body(size) {
        Ok(b) => b,
        Err(e @ ReadError::PayloadTooLarge) => {
            write(writer, HttpResponse::payload_too_large_err(e.to_string()));
            return;
        }
        Err(_) => {
            write(
                writer,
                HttpResponse::bad_request_err("Failed to read request body"),
            );
            return;
        }
    };

    let content_type = match validate_content_type(&headers) {
        Ok(value) => value,
        Err(e) => {
            write(writer, HttpResponse::bad_request_err(e.to_string()));
            return;
        }
    };

    if content_type.is_none() {
        write(
            writer,
            HttpResponse::bad_request_err(format!("'{}' is required", CONTENT_TYPE)),
        );
        return;
    }

    handle(request_line, headers, writer, store, Some(body));
}
