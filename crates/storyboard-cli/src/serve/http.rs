//! HTTP plumbing: request bodies, responses, loopback guards and URL decoding.

use std::{
    collections::HashMap,
    io::{self, Read},
};

use serde_json::Value;
use tiny_http::{Header, Request, Response};

use crate::ops_api::{self, ApiError};

/// Largest request body accepted: a batch of ops, never a file upload.
const MAX_BODY: u64 = 8 * 1024 * 1024;

/// A binary response: a rendered image or a JSON error.
pub(super) struct Bytes {
    pub(super) status: u16,
    pub(super) content_type: &'static str,
    pub(super) body: Vec<u8>,
    pub(super) extra: Vec<Header>,
}

pub(super) fn read_body(request: &mut Request) -> Result<Value, ApiError> {
    let bytes = read_bytes(request, MAX_BODY)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| ops_api::validation(format!("request body is not JSON: {error}")))
}

pub(super) fn read_bytes(request: &mut Request, max: u64) -> Result<Vec<u8>, ApiError> {
    let mut bytes = Vec::new();
    request
        .as_reader()
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ops_api::validation(format!("request body: {error}")))?;
    if bytes.len() as u64 > max {
        return Err(ops_api::validation(format!(
            "request body is larger than {} MB",
            max / 1024 / 1024
        )));
    }
    Ok(bytes)
}

pub(super) fn status_for(value: &Value) -> u16 {
    match value.pointer("/error/kind").and_then(Value::as_str) {
        Some("conflict") => 409,
        Some("not_found") => 404,
        Some("validation") | Some("usage") | Some("authorship") | Some("empty_commit") => 400,
        Some("render") | Some("compile") => 422,
        _ => 500,
    }
}

pub(super) fn respond_json(request: Request, result: Result<Value, ApiError>) {
    let (status, body) = match result {
        Ok(value) => (200, value),
        Err(error) => {
            let value = ops_api::map_error(error);
            (status_for(&value), value)
        }
    };
    let response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json"))
        .with_header(header("Cache-Control", "no-store"));
    let _ = request.respond(response);
}

pub(super) fn respond_bytes(request: Request, bytes: Bytes) {
    let mut response = Response::from_data(bytes.body)
        .with_status_code(bytes.status)
        .with_header(header("Content-Type", bytes.content_type));
    for extra in bytes.extra {
        response = response.with_header(extra);
    }
    let _ = request.respond(response);
}

pub(super) fn error_bytes(error: ApiError) -> Bytes {
    let value = ops_api::map_error(error);
    Bytes {
        status: status_for(&value),
        content_type: "application/json",
        body: value.to_string().into_bytes(),
        extra: vec![header("Cache-Control", "no-store")],
    }
}

pub(super) fn asset(body: &'static [u8], content_type: &str) -> Response<io::Cursor<Vec<u8>>> {
    Response::from_data(body)
        .with_header(header("Content-Type", content_type))
        .with_header(header("Cache-Control", "no-cache"))
        .with_header(header("X-Content-Type-Options", "nosniff"))
}

pub(super) fn text_response(status: u16, body: &str) -> Response<io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(status)
        .with_header(header("Content-Type", "text/plain; charset=utf-8"))
}

pub(super) fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap_or_else(|_| {
        Header::from_bytes(&b"X-Invalid"[..], &b"1"[..]).expect("static header")
    })
}

pub(super) fn header_value<'a>(request: &'a Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str())
}

/// Refuses requests whose Host is not this loopback server, which blocks DNS rebinding.
pub(super) fn host_is_local(request: &Request, port: u16) -> bool {
    match header_value(request, "Host") {
        Some(host) => local_authority(host, port),
        None => false,
    }
}

/// Writes must come from the page itself (or a client that sends no Origin, such as curl).
pub(super) fn origin_is_local(request: &Request, port: u16) -> bool {
    match header_value(request, "Origin") {
        None => true,
        Some(origin) => origin
            .strip_prefix("http://")
            .is_some_and(|authority| local_authority(authority, port)),
    }
}

pub(super) fn local_authority(authority: &str, port: u16) -> bool {
    [format!("127.0.0.1:{port}"), format!("localhost:{port}")]
        .iter()
        .any(|allowed| authority.eq_ignore_ascii_case(allowed))
}

pub(super) fn split_url(url: &str) -> (String, HashMap<String, String>) {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let pairs = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key), decode(value))
        })
        .collect();
    (path.to_string(), pairs)
}

pub(super) fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let escaped = (byte == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (byte, escaped) {
            (_, Some(decoded)) => {
                out.push(decoded);
                index += 3;
            }
            (b'+', None) => {
                out.push(b' ');
                index += 1;
            }
            (other, None) => {
                out.push(other);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(super) fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_query_values() {
        let (path, query) =
            split_url("/api/project?project=night%20station&cut=&x=a+b&bad=%zz&end=%4");
        assert_eq!(path, "/api/project");
        assert_eq!(query["project"], "night station");
        assert_eq!(query["cut"], "");
        assert_eq!(query["x"], "a b");
        assert_eq!(query["bad"], "%zz");
        assert_eq!(query["end"], "%4");
    }

    #[test]
    fn only_loopback_authorities_are_local() {
        assert!(local_authority("127.0.0.1:7420", 7420));
        assert!(local_authority("localhost:7420", 7420));
        assert!(!local_authority("evil.example:7420", 7420));
        assert!(!local_authority("127.0.0.1:80", 7420));
    }
}
