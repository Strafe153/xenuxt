use std::{
    collections::HashMap,
    error::Error,
    fmt::{self, Display, Formatter},
};

pub const CRLF: &'static str = "\r\n";
pub const CONTENT_LENGTH: &'static str = "Content-Length";

const HTTP_VERSION: &'static str = "HTTP/1.1";
const HEADER_VALUE_MAX_LENGTH: usize = 8192;
const CONTENT_TYPE: &'static str = "Content-Type";
const APPLICATION_JSON: &'static str = "application/json";

// Obviously, this primitive server implementation does NOT directly acknowledge a huge number of headers
// however the majority will be passed down to the handlers, while these are mentioned specifically because:
// - Transfer-Encoding should NOT be sent with Content-Length, which is required here
// - Connection specifies the connection options, which are NOT implemented
// - Upgrade is used for transitioning between protocols, however only HTTP/1.1 is supported
const UNSUPPORTED_HEADERS: [&str; 3] = ["Transfer-Encoding", "Connection", "Upgrade"];

// potentially unify all http errors under and enum
#[derive(Debug)]
pub struct HttpParseError(String);

impl HttpParseError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for HttpParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for HttpParseError {}

// A small representation of some of the most popular status codes,
// excluding any Information and Redirect codes
pub enum HttpStatusCode {
    OK,
    Created,
    NoContent,
    BadRequest,
    NotFound,
    MethodNotAllowed,
    LengthRequired,
}

impl HttpStatusCode {
    fn to_response_line(&self) -> Vec<u8> {
        format!("{} {} {}{}", HTTP_VERSION, self.code(), self.text(), CRLF)
            .as_bytes()
            .to_vec()
    }

    fn code(&self) -> u16 {
        match self {
            HttpStatusCode::OK => 200,
            HttpStatusCode::Created => 201,
            HttpStatusCode::NoContent => 204,
            HttpStatusCode::BadRequest => 400,
            HttpStatusCode::NotFound => 404,
            HttpStatusCode::MethodNotAllowed => 405,
            HttpStatusCode::LengthRequired => 411,
        }
    }

    fn text(&self) -> &'static str {
        match self {
            HttpStatusCode::OK => "OK",
            HttpStatusCode::Created => "Created",
            HttpStatusCode::NoContent => "No Content",
            HttpStatusCode::BadRequest => "Bad Request",
            HttpStatusCode::NotFound => "Not Found",
            HttpStatusCode::MethodNotAllowed => "Method Not Allowed",
            HttpStatusCode::LengthRequired => "Length Required",
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    PATCH,
    DELETE,
    HEAD,
}

impl TryFrom<&str> for HttpMethod {
    type Error = HttpParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "GET" => Ok(HttpMethod::GET),
            "POST" => Ok(HttpMethod::POST),
            "PUT" => Ok(HttpMethod::PUT),
            "PATCH" => Ok(HttpMethod::PATCH),
            "DELETE" => Ok(HttpMethod::DELETE),
            "HEAD" => Ok(HttpMethod::HEAD),
            _ => Err(HttpParseError::new("Invalid HTTP method")),
        }
    }
}

impl From<&HttpMethod> for &'static str {
    fn from(value: &HttpMethod) -> Self {
        match value {
            HttpMethod::GET => "GET",
            HttpMethod::POST => "POST",
            HttpMethod::PUT => "PUT",
            HttpMethod::PATCH => "PATCH",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::HEAD => "HEAD",
        }
    }
}

pub struct HttpRequestLine {
    pub query_string: Option<String>,
    method: HttpMethod,
    target: String,
}

impl HttpRequestLine {
    pub fn requires_body(&self) -> bool {
        match self.method {
            HttpMethod::POST | HttpMethod::PUT | HttpMethod::PATCH => true,
            _ => false,
        }
    }

    fn validate_parameter<'a>(
        value: Option<&'a str>,
        name: impl Display,
    ) -> Result<&'a str, HttpParseError> {
        match value {
            Some(v) if !v.is_empty() => Ok(v),
            Some(_) => Err(HttpParseError::new(format!("{} MUST NOT be empty", name))),
            None => Err(HttpParseError::new(format!("{} is required", name))),
        }
    }
}

impl TryFrom<String> for HttpRequestLine {
    type Error = HttpParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !value.ends_with(CRLF) {
            return Err(HttpParseError::new("Request line does NOT end with CRLF."));
        }

        let mut split = value.trim_end_matches(CRLF).split(' ');

        let method = HttpRequestLine::validate_parameter(split.next(), "Method")?;
        let mut target = HttpRequestLine::validate_parameter(split.next(), "Target")?;
        let version = HttpRequestLine::validate_parameter(split.next(), "Version")?;

        if !target.starts_with('/') {
            return Err(HttpParseError::new("Request target MUST start with a '/'"));
        }

        let mut query_string: Option<String> = None;

        // Current implementation simply passes the query string downstream,
        // offloading the parsing and validation onto the user
        if let Some((path, query)) = target.split_once('?') {
            target = path;
            query_string = Some(query.to_string());
        }

        if split.next().is_some() {
            return Err(HttpParseError::new("Request line MUST consist of 3 parts."));
        }

        let method = HttpMethod::try_from(method)?;

        if version != HTTP_VERSION {
            return Err(HttpParseError::new(format!(
                "Only {} version is supported.",
                HTTP_VERSION
            )));
        }

        Ok(HttpRequestLine {
            method,
            target: target.to_string(),
            query_string,
        })
    }
}

#[derive(Debug)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

impl HttpHeader {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

impl TryFrom<String> for HttpHeader {
    type Error = HttpParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let value = value.trim_end_matches(CRLF);

        let Some((name, value)) = value.split_once(':') else {
            return Err(HttpParseError::new(
                "Header must contain name and value separate by a colon. Obsolete line folding is not allowed",
            ));
        };

        if name.ends_with(' ') {
            return Err(HttpParseError::new(
                "Header name cannot contain trailing spaces",
            ));
        }

        let value = value.trim_matches([' ', '\t']);

        if value.is_empty() {
            return Err(HttpParseError::new("Header value cannot be empty"));
        }

        if value.len() > HEADER_VALUE_MAX_LENGTH {
            return Err(HttpParseError(format!(
                "Header value cannot be longer than {}",
                HEADER_VALUE_MAX_LENGTH
            )));
        }

        Ok(HttpHeader {
            name: name.to_string(),
            value: value.to_string(),
        })
    }
}

impl<'a> From<HttpHeader> for Vec<u8> {
    fn from(value: HttpHeader) -> Self {
        format!("{}: {}{}", value.name, value.value, CRLF).into_bytes()
    }
}

const HOST_HEADER: &'static str = "Host";
const LOCALHOST: &'static str = "localhost";
const LOCALHOST_IP_V4: &'static str = "127.0.0.1";
fn validate_host_header(headers: &[HttpHeader], port: u16) -> Result<(), HttpValidationError> {
    let host_count = headers
        .iter()
        .filter(|&h| h.name.as_str().eq_ignore_ascii_case(HOST_HEADER))
        .count();

    if host_count > 1 {
        return Err(HttpValidationError::new(format!(
            "Only one '{}' header is allowed",
            HOST_HEADER
        )));
    }

    let host = headers
        .iter()
        .find(|&h| h.name.as_str().eq_ignore_ascii_case(HOST_HEADER));

    match host {
        Some(h) => {
            if !h.value.starts_with(LOCALHOST) && !h.value.starts_with(LOCALHOST_IP_V4) {
                return Err(HttpValidationError::new(format!(
                    "Host MUST be either {} or {}",
                    LOCALHOST, LOCALHOST_IP_V4
                )));
            }

            match h.value.split_once(':') {
                Some((_, p)) => {
                    match p.parse::<u16>() {
                        Ok(p) => {
                            if p != port {
                                return Err(HttpValidationError::new("Incorrect port"));
                            }

                            return Ok(());
                        }
                        Err(_) => {
                            return Err(HttpValidationError::new(""));
                        }
                    };
                }
                None => {
                    if port != 80 {
                        return Err(HttpValidationError::new("Incorrect port"));
                    }

                    Ok(())
                }
            }
        }
        None => {
            return Err(HttpValidationError::new("Host header is required"));
        }
    }
}

fn validate_unsupported_headers(headers: &[HttpHeader]) -> Result<(), HttpValidationError> {
    let unsupported_header = headers
        .iter()
        .find(|h| UNSUPPORTED_HEADERS.contains(&h.name.as_str()));

    if let Some(h) = unsupported_header {
        return Err(HttpValidationError::new(format!(
            "{} is NOT supported",
            h.name
        )));
    }

    Ok(())
}

fn validate_content_type_header(headers: &[HttpHeader]) -> Result<(), HttpValidationError> {
    let header = headers
        .iter()
        .find(|&h| h.name.as_str().eq_ignore_ascii_case(CONTENT_TYPE));

    match header {
        Some(header) => {
            if header.name == CONTENT_TYPE && header.value != APPLICATION_JSON {
                return Err(HttpValidationError::new(format!(
                    "Only '{}' is supported for '{}'",
                    APPLICATION_JSON, CONTENT_TYPE
                )));
            }

            return Ok(());
        }
        None => {}
    }

    Ok(())
}

#[derive(Debug)]
pub struct HttpValidationError(String);

impl HttpValidationError {
    fn new(error: impl Into<String>) -> Self {
        Self(error.into())
    }
}

impl Display for HttpValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for HttpValidationError {}

// most likely refactor, since I don't like how validation works as of now
pub fn validate_headers(headers: &[HttpHeader], port: u16) -> Result<(), HttpValidationError> {
    validate_host_header(headers, port)?;
    validate_unsupported_headers(headers)?;
    validate_content_type_header(headers)?;

    Ok(())
}

pub struct HttpBody(Vec<u8>);

impl HttpBody {
    pub fn new(value: Vec<u8>) -> Self {
        Self(value)
    }

    pub fn value(self) -> Vec<u8> {
        self.0
    }
}

pub struct HttpResponse {
    status: HttpStatusCode,
    headers: Option<Vec<HttpHeader>>,
    body: Option<Vec<u8>>,
}

impl HttpResponse {
    pub fn ok(headers: Option<Vec<HttpHeader>>, body: Option<Vec<u8>>) -> Self {
        Self::new(HttpStatusCode::OK, headers, body)
    }

    pub fn created(headers: Option<Vec<HttpHeader>>, body: Option<Vec<u8>>) -> Self {
        Self::new(HttpStatusCode::Created, headers, body)
    }

    pub fn no_content(headers: Option<Vec<HttpHeader>>) -> Self {
        Self::new(HttpStatusCode::NoContent, headers, None)
    }

    pub fn bad_request(headers: Option<Vec<HttpHeader>>, body: Option<Vec<u8>>) -> Self {
        Self::new(HttpStatusCode::BadRequest, headers, body)
    }

    pub fn bad_request_err(error: impl Into<String>) -> Self {
        let body = format!("{{\r\n\t\"error\": \"{}\"\r\n}}\r\n", error.into())
            .as_bytes()
            .to_vec();

        Self::new(HttpStatusCode::BadRequest, None, Some(body))
    }

    pub fn not_found(headers: Option<Vec<HttpHeader>>, body: Option<Vec<u8>>) -> Self {
        Self::new(HttpStatusCode::NotFound, headers, body)
    }

    pub fn method_not_allowed(methods: Vec<HttpMethod>) -> Self {
        let methods = methods
            .iter()
            .map(|m| m.into())
            .collect::<Vec<&'static str>>()
            .join(", ");

        let headers = vec![HttpHeader::new("Allow", methods)];

        Self::new(HttpStatusCode::MethodNotAllowed, Some(headers), None)
    }

    pub fn length_required() -> HttpResponse {
        HttpResponse::new(HttpStatusCode::LengthRequired, None, None)
    }

    fn new(
        status: HttpStatusCode,
        headers: Option<Vec<HttpHeader>>,
        body: Option<Vec<u8>>,
    ) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }
}

impl From<HttpResponse> for Vec<u8> {
    fn from(value: HttpResponse) -> Self {
        let mut response = Vec::new();

        let response_line = value.status.to_response_line();
        response.extend_from_slice(&response_line);

        value
            .headers
            .into_iter()
            .flatten()
            .for_each(|h| response.extend_from_slice(&Vec::<u8>::from(h)));

        match value.body {
            Some(b) => {
                response.extend_from_slice(b"Content-Type: application/json\r\n");

                let content_length = format!("{}: {}{}", CONTENT_LENGTH, b.len(), CRLF);
                response.extend_from_slice(&content_length.as_bytes());

                response.extend_from_slice(CRLF.as_bytes());
                response.extend_from_slice(&b);
            }
            None => {
                response.extend_from_slice(b"Content-Length: 0\r\n");
                response.extend_from_slice(CRLF.as_bytes());
            }
        }

        response
    }
}

pub struct RequestPayload {
    pub headers: Option<Vec<HttpHeader>>,
    pub body: Option<Vec<u8>>,
    pub query_string: Option<String>,
}

impl RequestPayload {
    pub fn new(
        headers: Option<Vec<HttpHeader>>,
        body: Option<Vec<u8>>,
        query_string: Option<String>,
    ) -> Self {
        Self {
            headers,
            body,
            query_string,
        }
    }
}

type HttpHandlerFn = dyn 'static + Fn(RequestPayload) -> HttpResponse;

struct HandlerInfo {
    method: HttpMethod,
    handler: Box<HttpHandlerFn>,
}

pub enum HttpHandler<'a> {
    Found(&'a HttpHandlerFn),
    MethodNotAllowed(Vec<HttpMethod>),
    NotFound,
}

#[derive(Debug)]
pub struct HandlerRegistrationError(String);

impl HandlerRegistrationError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for HandlerRegistrationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for HandlerRegistrationError {}

pub struct HttpHandlerStore(HashMap<String, Vec<HandlerInfo>>);

impl HttpHandlerStore {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn register<F>(
        &mut self,
        path: impl Into<String>,
        method: HttpMethod,
        handler: F,
    ) -> Result<(), HandlerRegistrationError>
    where
        F: 'static + Fn(RequestPayload) -> HttpResponse,
    {
        let path = path.into();

        if !path.starts_with('/') {
            return Err(HandlerRegistrationError::new(
                "The path must start with '/'",
            ));
        }

        let handler_exists = self.0.iter().any(|(p, handlers)| {
            let same_method_handler_exists = handlers.iter().any(|i| i.method == method);
            return *p == path && same_method_handler_exists;
        });

        if handler_exists {
            return Err(HandlerRegistrationError::new(
                "There is already a handler for this path",
            ));
        }

        let info = HandlerInfo {
            method,
            handler: Box::new(handler),
        };

        match self.0.get_mut(&path) {
            Some(v) => v.push(info),
            None => {
                self.0.insert(path, vec![info]);
            }
        }

        Ok(())
    }

    pub fn get(&self, req: &HttpRequestLine) -> HttpHandler<'_> {
        match self.0.get(&req.target) {
            Some(handlers) => {
                let handler = handlers.iter().find(|h| h.method == req.method);

                match handler {
                    Some(info) => return HttpHandler::Found(&info.handler),
                    None => {
                        let methods: Vec<HttpMethod> = handlers.iter().map(|i| i.method).collect();
                        return HttpHandler::MethodNotAllowed(methods);
                    }
                }
            }
            None => return HttpHandler::NotFound,
        }
    }
}
