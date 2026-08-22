use std::{error::Error, fmt::{self, Display, Formatter}};

const CRLF: &'static str = "\r\n";
const HTTP_VERSION: &'static str = "HTTP/1.1";
const HEADER_VALUE_MAX_LENGTH: usize = 8192;

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

#[derive(Debug)]
enum HttpMethod {
    GET,
    POST,
    PUT,
    PATCH,
    DELETE,
    HEAD,
    OPTIONS,
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
            "OPTIONS" => Ok(HttpMethod::OPTIONS),
            _ => Err(HttpParseError::new("Invalid HTTP method.")),
        }
    }
}

#[derive(Debug)]
pub struct HttpRequestLine {
    method: HttpMethod,
    target: String,
}

impl HttpRequestLine {
    fn validate_parameter<'a>(
        value: Option<&'a str>,
        name: impl Display,
    ) -> Result<&'a str, HttpParseError> {
        match value {
            Some(v) if !v.is_empty() => Ok(v),
            Some(_) => Err(HttpParseError::new(format!("{} MUST NOT be empty.", name))),
            None => Err(HttpParseError::new(format!("{} is required.", name))),
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
    name: String,
    value: String,
}

impl TryFrom<String> for HttpHeader {
    type Error = HttpParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let value = value.trim_end_matches(CRLF);

        // this also handles line folding - should return 400 saying that obsolete line folding is unacceptable,
        // since it's been deprecated everywhere other than in message/http
        let Some((name, value)) = value.split_once(':') else {
            return Err(HttpParseError::new(
                "Header must contain name and value separate by a colon",
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
