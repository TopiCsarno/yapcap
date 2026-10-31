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
            .append_pair("state", &state);
        Ok(Self {
            listener,
            state,
            url: url.to_string(),
        })
    }

    pub async fn wait(self) -> Result<String, String> {
        tokio::time::timeout(Duration::from_secs(300), async {
            loop {
                let (mut stream, _) = self
                    .listener
                    .accept()
                    .await
                    .map_err(|_| "Could not receive browser callback".to_string())?;
                match tokio::time::timeout(
                    Duration::from_secs(5),
                    callback::handle(&mut stream, &self.state),
                )
                .await
                {
                    Ok(Ok(Some(key))) => return Ok(key),
                    Ok(Err(error)) => return Err(error),
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| {
            "Command Code browser login timed out; retry or paste an API key".to_string()
        })?
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

#[cfg(test)]
mod tests;

fn state_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| {
            "Secure randomness is unavailable; browser login was not started".to_string()
        })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
