// SPDX-License-Identifier: MPL-2.0

use super::callback;
use std::io::Read;
use std::time::Duration;
use tokio::net::TcpListener;

pub(super) struct BrowserLogin {
    listener: TcpListener,
    state: String,
    pub url: String,
}

impl BrowserLogin {
    pub async fn start() -> Result<Self, String> {
        let state = state_token()?;
        let listener = listener().await?;
        let port = listener
            .local_addr()
            .map_err(|_| "Could not inspect browser callback listener".to_string())?
            .port();
        let mut url = reqwest::Url::parse("https://commandcode.ai/studio/auth/cli")
            .map_err(|_| "Could not build Command Code login URL".to_string())?;
        url.query_pairs_mut()
            .append_pair("callback", &format!("http://127.0.0.1:{port}/callback"))
            .append_pair("state", &state)
            .append_pair("mode", "redirect");
        Ok(Self {
            listener,
            state,
            url: url.to_string(),
        })
    }

    pub async fn wait(self) -> Result<String, String> {
        tokio::time::timeout(
            Duration::from_secs(300),
            self.receive(Duration::from_secs(10)),
        )
        .await
        .map_err(|_| {
            "Command Code browser login timed out; retry or paste an API key".to_string()
        })?
    }

    async fn receive(self, completion_timeout: Duration) -> Result<String, String> {
        let mut pending = None;
        loop {
            let next = next_callback(&self.listener, &self.state, pending.is_some());
            let action = if let Some((_, deadline)) = &pending {
                match tokio::time::timeout_at(*deadline, next).await {
                    Ok(result) => result?,
                    Err(_) => return completed_key(pending),
                }
            } else {
                next.await?
            };
            match action {
                callback::CallbackAction::Ignored => {}
                callback::CallbackAction::Credential(key) => return Ok(key),
                callback::CallbackAction::Redirect(key) => {
                    pending = Some((key, tokio::time::Instant::now() + completion_timeout));
                }
                callback::CallbackAction::Complete => return completed_key(pending),
            }
        }
    }
}

async fn listener() -> Result<TcpListener, String> {
    for port in 5959..5969 {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(_) => {
                return Err(
                    "Could not start Command Code browser login; paste an API key instead"
                        .to_string(),
                );
            }
        }
    }
    TcpListener::bind(("127.0.0.1", 0)).await.map_err(|_| {
        "Could not start Command Code browser login; paste an API key instead".to_string()
    })
}

async fn next_callback(
    listener: &TcpListener,
    state: &str,
    can_complete: bool,
) -> Result<callback::CallbackAction, String> {
    let (mut stream, _) = listener
        .accept()
        .await
        .map_err(|_| "Could not receive browser callback".to_string())?;
    tokio::time::timeout(
        Duration::from_secs(5),
        callback::handle(&mut stream, state, can_complete),
    )
    .await
    .unwrap_or(Ok(callback::CallbackAction::Ignored))
}

fn completed_key(pending: Option<(String, tokio::time::Instant)>) -> Result<String, String> {
    pending
        .map(|(key, _)| key)
        .ok_or_else(|| "Command Code browser login did not complete".to_string())
}

fn state_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| {
            "Secure randomness is unavailable; browser login was not started".to_string()
        })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests;
