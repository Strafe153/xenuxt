use std::{
    collections::HashMap,
    error::Error,
    fmt::{self, Display, Formatter},
};

pub const CRLF: &'static str = "\r\n";
pub const CONTENT_LENGTH: &'static str = "Content-Length";

const HTTP_VERSION: &'static str = "HTTP/1.1";
const HEADER_VALUE_MAX_LENGTH: usize = 8192;
const NOT_SUPPORTED_HEADERS: [&str; 1] = ["Transfer-Encoding"];
const CONTENT_TYPE: &'static str = "Content-Type";
const APPLICATION_JSON: &'static str = "application/json";
const APPLICATION_JSON_CONTENT_TYPE: &'static [u8; 32] = b"Content-Type: application/json\r\n";
const EMPTY_CONTENT_LENGTH: &'static [u8; 19] = b"Content-Length: 0\r\n";

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

#[derive(PartialEq)]
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
        let target = HttpRequestLine::validate_parameter(split.next(), "Target")?;
        let version = HttpRequestLine::validate_parameter(split.next(), "Version")?;

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

        if NOT_SUPPORTED_HEADERS
            .iter()
            .any(|&h| h.eq_ignore_ascii_case(name))
        {
            return Err(HttpParseError::new(format!(
                "Header \"{}\" is not supported",
                name
            )));
        }

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

        if name == CONTENT_TYPE && value != APPLICATION_JSON {
            return Err(HttpParseError::new(format!(
                "Only \"{}\" is supported for \"{}\"",
                APPLICATION_JSON, CONTENT_TYPE
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

    pub fn method_not_allowed(method: &HttpMethod) -> Self {
        let method: &'static str = method.into();
        let headers = vec![HttpHeader::new("Allow", method)];

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
                response.extend_from_slice(APPLICATION_JSON_CONTENT_TYPE);

                let content_length = format!("{}: {}{}", CONTENT_LENGTH, b.len(), CRLF);
                response.extend_from_slice(&content_length.as_bytes());

                response.extend_from_slice(CRLF.as_bytes());
                response.extend_from_slice(&b);
            }
            None => {
                response.extend_from_slice(EMPTY_CONTENT_LENGTH);
                response.extend_from_slice(CRLF.as_bytes());
            }
        }

        response
    }
}

type HttpHandlerFn = dyn 'static + Fn(Option<Vec<HttpHeader>>, Option<Vec<u8>>) -> HttpResponse;

pub struct HttpHandlerInfo {
    method: HttpMethod,
    handler: Box<HttpHandlerFn>,
}

pub enum HttpHandler<'a> {
    Found(&'a HttpHandlerFn),
    MethodNotAllowed(&'a HttpMethod),
    NotFound,
}

pub struct HttpHandlerStore(HashMap<String, HttpHandlerInfo>);

impl HttpHandlerStore {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn register<F>(
        &mut self,
        path: impl Into<String>,
        method: HttpMethod,
        handler: F,
    ) -> Result<(), std::io::Error>
    where
        F: 'static + Fn(Option<Vec<HttpHeader>>, Option<Vec<u8>>) -> HttpResponse,
    {
        let path = path.into();

        // allow having several handlers for the same path with different methods - for that change value to Vec<HttpHandlerInfo>
        // use a proper error instead of this placeholder
        if self.0.iter().any(|h| *h.0 == path) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "There is already a handler for this path",
            ));
        }

        let info = HttpHandlerInfo {
            method,
            handler: Box::new(handler),
        };

        self.0.insert(path, info);

        Ok(())
    }

    pub fn get(&self, req: HttpRequestLine) -> HttpHandler<'_> {
        match self.0.get(&req.target) {
            Some(h) if req.method != h.method => HttpHandler::MethodNotAllowed(&h.method),
            Some(h) => HttpHandler::Found(&h.handler),
            None => HttpHandler::NotFound,
        }
    }
}
