use crate::error::{ClaudeError, Result};
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::Utc;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CLAUDE_STARTUP_DELAY: Duration = Duration::from_millis(1500);
const CLAUDE_TIMEOUT: Duration = Duration::from_secs(12);

pub async fn fetch() -> Result<UsageSnapshot> {
    tokio::task::spawn_blocking(fetch_blocking)
        .await
        .map_err(|_| ClaudeError::CliParse)?
}

fn fetch_blocking() -> Result<UsageSnapshot> {
    let transcript = run_usage_command()?;
    parse_usage_snapshot(&transcript)
}

fn run_usage_command() -> Result<String> {
    let mut child = Command::new("script")
        .args(["-qefc", "claude --allowed-tools \"\"", "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ClaudeError::CliUnavailable(source)
            } else {
                ClaudeError::CliCommand(source)
            }
        })?;

    let mut stdin = child.stdin.take().ok_or(ClaudeError::CliParse)?;
    thread::sleep(CLAUDE_STARTUP_DELAY);
    stdin.write_all(b"/usage\n").map_err(ClaudeError::CliIo)?;
    stdin.flush().map_err(ClaudeError::CliIo)?;
    drop(stdin);

    let deadline = Instant::now() + CLAUDE_TIMEOUT;
    loop {
        if child.try_wait().map_err(ClaudeError::CliCommand)?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ClaudeError::CliTimeout {
                timeout: CLAUDE_TIMEOUT,
            }
            .into());
        }
        thread::sleep(Duration::from_millis(200));
    }

    let output = child.wait_with_output().map_err(ClaudeError::CliCommand)?;
    let mut transcript = String::from_utf8_lossy(&output.stdout).to_string();
    transcript.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(transcript)
}

pub(crate) fn parse_usage_snapshot(transcript: &str) -> Result<UsageSnapshot> {
    let clean = strip_ansi(transcript);
    let primary =
        percent_for_any_label(&clean, &["current session", "session"]).map(|used_percent| {
            UsageWindow {
                label: "5h".to_string(),
                used_percent,
                reset_at: None,
                reset_description: None,
            }
        });
    let secondary =
        percent_for_any_label(&clean, &["current week", "weekly", "week"]).map(|used_percent| {
            UsageWindow {
                label: "7d".to_string(),
                used_percent,
                reset_at: None,
                reset_description: None,
            }
        });
    let extra_percent = percent_for_any_label(&clean, &["extra usage", "extra"]);
    let tertiary = extra_percent.map(|used_percent| UsageWindow {
        label: "Extra".to_string(),
        used_percent,
        reset_at: None,
        reset_description: None,
    });

    if primary.is_none() && secondary.is_none() && tertiary.is_none() {
        return Err(ClaudeError::CliParse.into());
    }

    Ok(UsageSnapshot {
        provider: ProviderId::Claude,
        source: "CLI".to_string(),
        updated_at: Utc::now(),
        headline: if secondary.is_some() {
            UsageHeadline::Secondary
        } else {
            UsageHeadline::Primary
        },
        primary,
        secondary,
        tertiary,
        provider_cost: extra_percent.map(|used| ProviderCost {
            used,
            limit: None,
            units: "%".to_string(),
        }),
        identity: ProviderIdentity::default(),
    })
}

fn percent_for_any_label(text: &str, labels: &[&str]) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    labels.iter().find_map(|label| {
        lower.find(label).and_then(|start| {
            let end = (start + 160).min(text.len());
            first_percent(&text[start..end])
        })
    })
}

fn first_percent(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut start = None;
    for (index, byte) in bytes.iter().enumerate() {
        if byte.is_ascii_digit() {
            start.get_or_insert(index);
            continue;
        }
        if *byte == b'.' && start.is_some() {
            continue;
        }
        if *byte == b'%' && start.is_some() {
            return text[start?..index].trim().parse::<f64>().ok();
        }
        start = None;
    }
    None
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars
                .peek()
                .is_some_and(|next| *next == '[' || *next == ']')
            {
                let _ = chars.next();
                while let Some(next) = chars.next() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        output.push(ch);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_usage_panel() {
        let transcript = "\
Current session 18%\n\
Current week 90%\n\
Extra usage 48.8%\n";
        let snapshot = parse_usage_snapshot(transcript).unwrap();
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 18.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 90.0);
        assert_eq!(snapshot.tertiary.as_ref().unwrap().used_percent, 48.8);
        assert_eq!(snapshot.source, "CLI");
    }

    #[test]
    fn parses_cli_fixture() {
        let transcript = include_str!("../../fixtures/claude/usage_cli.txt");
        let snapshot = parse_usage_snapshot(transcript).unwrap();
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 18.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 90.0);
        assert_eq!(snapshot.tertiary.as_ref().unwrap().used_percent, 48.8);
    }

    #[test]
    fn strips_ansi_before_parsing() {
        let transcript =
            "\u{1b}[2mCurrent session\u{1b}[0m 12%\n\u{1b}[2mCurrent week\u{1b}[0m 34%\n";
        let snapshot = parse_usage_snapshot(transcript).unwrap();
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 12.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 34.0);
    }
}
