// SPDX-License-Identifier: MPL-2.0

use super::account::normalize_key;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const ORIGIN: &str = "https://commandcode.ai";
const MAX_REQUEST_BYTES: usize = 16_384;

#[derive(Deserialize)]
struct Callback {
    state: String,
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
    error: Option<String>,
}

pub(super) async fn handle(
    stream: &mut TcpStream,
    expected_state: &str,
) -> Result<Option<String>, String> {
    let Some((headers, body)) = read_request(stream).await else {
        respond(stream, 400, false).await;
        return Ok(None);
    };
    let mut request_line = headers
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = request_line.next().unwrap_or_default();
    let path = request_line.next().unwrap_or_default();
    if path != "/callback" {
        respond(stream, 404, false).await;
        return Ok(None);
    }
    if header(&headers, "origin") != Some(ORIGIN) {
        respond(stream, 403, false).await;
        return Ok(None);
    }
    if method == "OPTIONS" {
        respond(stream, 204, true).await;
        return Ok(None);
    }
    if method != "POST" {
        respond(stream, 405, false).await;
        return Ok(None);
    }
    let Ok(callback) = serde_json::from_slice::<Callback>(&body) else {
        respond(stream, 400, false).await;
        return Ok(None);
    };
    if callback.state != expected_state {
        respond(stream, 403, false).await;
        return Ok(None);
    }
    if callback.error.is_some() {
        respond(stream, 200, true).await;
        return Err("Command Code authorization was denied or failed".to_string());
    }
    let Some(key) = callback.api_key.and_then(|key| normalize_key(&key).ok()) else {
        respond(stream, 400, false).await;
        return Ok(None);
    };
    respond(stream, 200, true).await;
    Ok(Some(key))
}

fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    let mut values = headers.lines().skip(1).filter_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    });
    let first = values.next()?;
    values.next().is_none().then_some(first)
}

async fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut request = Vec::new();
    let header_end = loop {
        if let Some(position) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break position + 4;
        }
        read_more(stream, &mut request).await?;
    };
    let headers = std::str::from_utf8(&request[..header_end])
        .ok()?
        .to_string();
    if headers.lines().any(|line| {
        line.split_once(':')
            .is_some_and(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
    }) {
        return None;
    }
    let body_length = match header(&headers, "content-length") {
        Some(value) => value.parse::<usize>().ok()?,
        None if headers.starts_with("OPTIONS ") => 0,
        None => return None,
    };
    let total_length = header_end.checked_add(body_length)?;
    if total_length > MAX_REQUEST_BYTES {
        return None;
    }
    while request.len() < total_length {
        read_more(stream, &mut request).await?;
    }
    Some((headers, request[header_end..total_length].to_vec()))
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

async fn respond(stream: &mut TcpStream, status: u16, success: bool) {
    let body = if status == 204 {
        ""
    } else if success {
        r#"{"success":true}"#
    } else {
        r#"{"success":false}"#
    };
    let response = format!(
        "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: {ORIGIN}\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nAccess-Control-Allow-Private-Network: true\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[cfg(test)]
mod tests;
