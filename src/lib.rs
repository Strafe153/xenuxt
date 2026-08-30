pub mod http;

mod reader;
mod writer;

use std::net::TcpListener;

use crate::{http::*, reader::Reader, writer::Writer};

pub fn run(store: HttpHandlerStore) {
    run_on_port(3017, store)
}

pub fn run_on_port(port: u16, store: HttpHandlerStore) {
    let listener = bind_listener(port);

    println!("Listening on port {}...", port);

    listen(listener, store, port);
}

fn bind_listener(port: u16) -> TcpListener {
    TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|_| panic!("Failed to bind to TCP port {}", port))
}

fn read_request_headers(reader: &mut Reader) -> Result<Vec<HttpHeader>, String> {
    let mut headers = Vec::new();

    loop {
        match reader.read_header() {
            Ok(header) => {
                if header == CRLF {
                    break;
                }

                match HttpHeader::try_from(header) {
                    Ok(h) => headers.push(h),
                    Err(e) => return Err(e.to_string()),
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }

    Ok(headers)
}

fn write(writer: &mut Writer, response: HttpResponse) {
    if let Err(_) = writer.write(response) {
        println!("Failed to write the response")
    }
}

pub fn listen(listener: TcpListener, store: HttpHandlerStore, port: u16) {
    // Add support for multiple clients and persistent connections
    // The first can be achieved with an inner loop for handling requests and responses
    // The second using a thread::spawn, I think
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

        let Ok(request_line) = reader.read_request_line() else {
            write(
                &mut writer,
                HttpResponse::bad_request_err("Failed to read the request line"),
            );
            continue;
        };

        let request_line = match HttpRequestLine::try_from(request_line) {
            Ok(line) => line,
            Err(e) => {
                write(&mut writer, HttpResponse::bad_request_err(e.to_string()));
                continue;
            }
        };

        let Ok(headers) = read_request_headers(&mut reader) else {
            write(
                &mut writer,
                HttpResponse::bad_request_err("Failed to read request headers"),
            );
            continue;
        };

        if let Err(e) = validate_headers(&headers, port) {
            write(&mut writer, HttpResponse::bad_request_err(e.to_string()));
            continue;
        }

        handle_request(request_line, headers, &mut reader, &mut writer, &store);
    }
}

fn handle_request(
    request_line: HttpRequestLine,
    headers: Vec<HttpHeader>,
    reader: &mut Reader,
    writer: &mut Writer,
    store: &HttpHandlerStore,
) {
    // most likely move  this into the header validation logic
    // with the validations returning headers or something
    let content_length = headers
        .iter()
        .find(|&h| h.name.as_str().eq_ignore_ascii_case(CONTENT_LENGTH));

    let Some(content_length) = content_length else {
        if request_line.requires_body() {
            write(writer, HttpResponse::length_required());
        }

        return;
    };

    let Ok(size) = content_length.value.parse::<usize>() else {
        write(
            writer,
            HttpResponse::bad_request_err(format!("'{}' value MUST be numeric", CONTENT_LENGTH)),
        );
        return;
    };

    let Ok(body) = reader.read_body(size) else {
        write(
            writer,
            HttpResponse::bad_request_err("Failed to read request body"),
        );
        return;
    };

    match store.get(&request_line) {
        HttpHandler::Found(handler) => {
            let body = HttpBody::new(body).value();
            let payload = RequestPayload::new(Some(headers), Some(body), request_line.query_string);

            write(writer, handler(payload));
        }
        HttpHandler::MethodNotAllowed(method) => {
            write(writer, HttpResponse::method_not_allowed(method))
        }
        HttpHandler::NotFound => write(writer, HttpResponse::not_found(None, None)),
    }
}
