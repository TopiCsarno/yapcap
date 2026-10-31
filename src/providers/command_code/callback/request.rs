// SPDX-License-Identifier: MPL-2.0

use std::collections::HashSet;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

const MAX_REQUEST_BYTES: usize = 16_384;

pub(super) struct Request {
    pub(super) method: String,
    pub(super) target: String,
    headers: String,
    pub(super) body: Vec<u8>,
}

impl Request {
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        self.headers.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then_some(value.trim_matches([' ', '\t']))
        })
    }
}

pub(super) async fn read(stream: &mut TcpStream) -> Option<Request> {
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(position) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break position + 4;
        }
        read_more(stream, &mut bytes).await?;
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).ok()?.to_string();
    validate_headers(&headers)?;
    let mut parts = headers.lines().next()?.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    if !matches!(parts.next()?, "HTTP/1.0" | "HTTP/1.1")
        || parts.next().is_some()
        || !target.starts_with('/')
        || target.starts_with("//")
    {
        return None;
    }
    let mut request = Request {
        method,
        target,
        headers,
        body: Vec::new(),
    };
    if request.header("transfer-encoding").is_some() {
        return None;
    }
    let body_length = match request.header("content-length") {
        Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
            value.parse::<usize>().ok()?
        }
        None if matches!(request.method.as_str(), "GET" | "OPTIONS") => 0,
        _ => return None,
    };
    let total = header_end.checked_add(body_length)?;
    if total > MAX_REQUEST_BYTES {
        return None;
    }
    while bytes.len() < total {
        read_more(stream, &mut bytes).await?;
    }
    request.body = bytes[header_end..total].to_vec();
    Some(request)
}

fn validate_headers(headers: &str) -> Option<()> {
    let mut names = HashSet::new();
    for line in headers.lines().skip(1).filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':')?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
            || value.bytes().any(|byte| byte < 32 && byte != b'\t')
            || !names.insert(name.to_ascii_lowercase())
        {
            return None;
        }
    }
    Some(())
}

async fn read_more(stream: &mut TcpStream, request: &mut Vec<u8>) -> Option<()> {
    let mut buffer = [0; 1024];
    let received = stream.read(&mut buffer).await.ok()?;
    if received == 0 || request.len() + received > MAX_REQUEST_BYTES {
        return None;
    }
    request.extend_from_slice(&buffer[..received]);
    Some(())
}
