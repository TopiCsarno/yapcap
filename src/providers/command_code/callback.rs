// SPDX-License-Identifier: MPL-2.0

mod request;
mod response;

use super::account::normalize_key;
use serde::Deserialize;
use tokio::net::TcpStream;

const ORIGIN: &str = "https://commandcode.ai";

pub(super) enum CallbackAction {
    Ignored,
    Credential(String),
    Redirect(String),
    Complete,
}

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
    can_complete: bool,
) -> Result<CallbackAction, String> {
    let Some(request) = request::read(stream).await else {
        response::json(stream, 400, false).await;
        return Ok(CallbackAction::Ignored);
    };
    let (path, query) = request
        .target
        .split_once('?')
        .unwrap_or((&request.target, ""));
    if path == "/callback/complete" && request.method == "GET" {
        return complete(stream, query, expected_state, can_complete).await;
    }
    if path != "/callback" {
        response::json(stream, 404, false).await;
        return Ok(CallbackAction::Ignored);
    }
    if request.method == "OPTIONS" && request.header("origin") == Some(ORIGIN) {
        response::json(stream, 204, true).await;
        return Ok(CallbackAction::Ignored);
    }
    if request.method != "POST" {
        response::json(stream, 405, false).await;
        return Ok(CallbackAction::Ignored);
    }
    if can_complete {
        response::json(stream, 409, false).await;
        return Ok(CallbackAction::Ignored);
    }
    post(stream, &request, expected_state).await
}

async fn post(
    stream: &mut TcpStream,
    request: &request::Request,
    expected_state: &str,
) -> Result<CallbackAction, String> {
    let content_type = request.header("content-type").unwrap_or_default();
    let content_type = content_type.split(';').next().unwrap_or_default().trim();
    let form = content_type.eq_ignore_ascii_case("application/x-www-form-urlencoded");
    if !form && !content_type.eq_ignore_ascii_case("application/json") {
        response::json(stream, 415, false).await;
        return Ok(CallbackAction::Ignored);
    }
    let origin = request.header("origin");
    if !(origin == Some(ORIGIN) || form && matches!(origin, None | Some("null"))) {
        response::json(stream, 403, false).await;
        return Ok(CallbackAction::Ignored);
    }
    let callback = if form {
        std::str::from_utf8(&request.body).ok().and_then(parse_form)
    } else {
        serde_json::from_slice::<Callback>(&request.body).ok()
    };
    let Some(callback) = callback else {
        response::json(stream, 400, false).await;
        return Ok(CallbackAction::Ignored);
    };
    if callback.state != expected_state {
        response::json(stream, 403, false).await;
        return Ok(CallbackAction::Ignored);
    }
    finish(stream, callback, form).await
}

async fn finish(
    stream: &mut TcpStream,
    callback: Callback,
    form: bool,
) -> Result<CallbackAction, String> {
    if callback.error.is_some() {
        if form {
            response::complete(stream, false).await;
        } else {
            response::json(stream, 200, true).await;
        }
        return Err("Command Code authorization was denied or failed".to_string());
    }
    let Some(key) = callback.api_key.and_then(|key| normalize_key(&key).ok()) else {
        response::json(stream, 400, false).await;
        return Ok(CallbackAction::Ignored);
    };
    if form {
        response::redirect(stream, &callback.state).await;
        Ok(CallbackAction::Redirect(key))
    } else {
        response::json(stream, 200, true).await;
        Ok(CallbackAction::Credential(key))
    }
}

async fn complete(
    stream: &mut TcpStream,
    query: &str,
    expected_state: &str,
    can_complete: bool,
) -> Result<CallbackAction, String> {
    if !can_complete
        || !parse_form(query).is_some_and(|callback| {
            callback.state == expected_state
                && callback.api_key.is_none()
                && callback.error.is_none()
        })
    {
        response::json(stream, 403, false).await;
        return Ok(CallbackAction::Ignored);
    }
    response::complete(stream, true).await;
    Ok(CallbackAction::Complete)
}

fn parse_form(body: &str) -> Option<Callback> {
    if body.bytes().any(|byte| byte.is_ascii_control()) {
        return None;
    }
    let mut url = reqwest::Url::parse("http://127.0.0.1").ok()?;
    url.set_query(Some(body));
    let (mut state, mut api_key, mut error) = (None, None, None);
    for (name, value) in url.query_pairs() {
        let field = match name.as_ref() {
            "state" => &mut state,
            "apiKey" => &mut api_key,
            "error" => &mut error,
            _ => continue,
        };
        if field.replace(value.into_owned()).is_some() {
            return None;
        }
    }
    Some(Callback {
        state: state?,
        api_key,
        error,
    })
}

#[cfg(test)]
mod tests;
