mod http;
mod reader;
mod writer;

use std::{env, net::TcpListener};

use crate::{http::*, reader::Reader, writer::Writer};

// TODO: add support for query and route parameters

fn main() {
    let port = get_port();

    let mut store = HttpHandlerStore::new();
    register_handlers(&mut store).expect("Failed to register handlers");

    listen(port, &store);
}

fn get_port() -> u16 {
    match env::args().nth(1) {
        Some(p) => match p.parse::<u16>() {
            Ok(p) => p,
            Err(_) => {
                panic!("Invalid port: {}", p);
            }
        },
        None => 1717,
    }
}

// maybe use serde_json for serialization
fn register_handlers(store: &mut HttpHandlerStore) -> Result<(), std::io::Error> {
    store.register("/", HttpMethod::GET, |headers, _| {
        HttpResponse::ok(
            headers,
            Some("{\r\n\t\"response\": \"test\"\r\n}\r\n".as_bytes().to_vec()),
        )
    })?;

    store.register("/bad", HttpMethod::POST, |headers, _| {
        HttpResponse::bad_request(
            headers,
            Some(
                "{\r\n\t\"response\": \"bad request\"\r\n}\r\n"
                    .as_bytes()
                    .to_vec(),
            ),
        )
    })?;

    Ok(())
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

fn bind_listener(port: u16) -> TcpListener {
    TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|_| panic!("Failed to bind to TCP port {}", port))
}

fn listen(port: u16, store: &HttpHandlerStore) {
    let listener = bind_listener(port);

    println!("Listening on port {}...", port);

    // add support for multiple clients
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

        let Ok(request_line) = HttpRequestLine::try_from(request_line) else {
            write(
                &mut writer,
                HttpResponse::bad_request_err("Failed to parse the request line"),
            );
            continue;
        };

        match read_request_headers(&mut reader) {
            Ok(headers) => {
                handle_request(request_line, headers, &mut reader, &mut writer, &store);
            }
            Err(e) => {
                write(&mut writer, HttpResponse::bad_request_err(e));
            }
        }
    }
}

fn handle_request(
    request_line: HttpRequestLine,
    headers: Vec<HttpHeader>,
    reader: &mut Reader,
    writer: &mut Writer,
    store: &HttpHandlerStore,
) {
    let content_length = headers
        .iter()
        .find(|&h| h.name.as_str().eq_ignore_ascii_case(CONTENT_LENGTH_HEADER));

    let Some(content_length) = content_length else {
        if request_line.requires_body() {
            write(writer, HttpResponse::length_required());
        }

        return;
    };

    let Ok(size) = content_length.value.parse::<usize>() else {
        write(
            writer,
            HttpResponse::bad_request_err(format!(
                "\"{}\" value MUST be numeric",
                CONTENT_LENGTH_HEADER
            )),
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

    match store.get(request_line) {
        HttpHandler::Found(handler) => {
            let body = HttpBody::new(body).value();
            write(writer, handler(Some(headers), Some(body)));
        }
        HttpHandler::MethodNotAllowed(method) => {
            write(writer, HttpResponse::method_not_allowed(method))
        }
        HttpHandler::NotFound => write(writer, HttpResponse::not_found(None, None)),
    }
}
