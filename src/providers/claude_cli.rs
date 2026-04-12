use crate::error::{ClaudeError, Result};
use crate::model::{
    ProviderCost, ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow,
};
use chrono::Utc;
use serde::Deserialize;
use std::process::{Command, Stdio};

const USAGE_PROMPT: &str = "Return only JSON with this exact shape: \
{\"session_percent\": number|null, \"week_percent\": number|null, \"extra_usage_percent\": number|null}. \
Use percentages from my current Claude usage. Do not include markdown, explanations, or code fences.";

#[derive(Debug, Deserialize)]
struct ClaudeUsageReply {
    session_percent: Option<f64>,
    week_percent: Option<f64>,
    extra_usage_percent: Option<f64>,
}

pub async fn fetch() -> Result<UsageSnapshot> {
    tokio::task::spawn_blocking(fetch_blocking)
        .await
        .map_err(|_| ClaudeError::CliParse)?
}

fn fetch_blocking() -> Result<UsageSnapshot> {
    let raw = run_print_command()?;
    parse_usage_snapshot(&raw)
}

fn run_print_command() -> Result<String> {
    let output = Command::new("claude")
        .arg("-p")
        .arg(USAGE_PROMPT)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ClaudeError::CliUnavailable(source)
            } else {
                ClaudeError::CliCommand(source)
            }
        })?;

    let mut transcript = String::from_utf8_lossy(&output.stdout).to_string();
    transcript.push_str(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        return Err(ClaudeError::CliParse.into());
    }

    Ok(transcript)
}

pub(crate) fn parse_usage_snapshot(transcript: &str) -> Result<UsageSnapshot> {
    let reply = parse_cli_json(transcript)?;

    let primary = reply.session_percent.map(|used_percent| UsageWindow {
        label: "5h".to_string(),
        used_percent,
        reset_at: None,
        reset_description: None,
    });
    let secondary = reply.week_percent.map(|used_percent| UsageWindow {
        label: "7d".to_string(),
        used_percent,
        reset_at: None,
        reset_description: None,
    });
    let tertiary = reply.extra_usage_percent.map(|used_percent| UsageWindow {
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
        provider_cost: reply.extra_usage_percent.map(|used| ProviderCost {
            used,
            limit: None,
            units: "%".to_string(),
        }),
        identity: ProviderIdentity::default(),
    })
}

fn parse_cli_json(transcript: &str) -> Result<ClaudeUsageReply> {
    let trimmed = transcript.trim();
    if let Ok(reply) = serde_json::from_str::<ClaudeUsageReply>(trimmed) {
        return Ok(reply);
    }

    let start = trimmed.find('{').ok_or(ClaudeError::CliParse)?;
    let end = trimmed.rfind('}').ok_or(ClaudeError::CliParse)?;
    serde_json::from_str::<ClaudeUsageReply>(&trimmed[start..=end])
        .map_err(|_| ClaudeError::CliParse.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_json_usage() {
        let transcript = r#"{"session_percent":18,"week_percent":90,"extra_usage_percent":48.8}"#;
        let snapshot = parse_usage_snapshot(transcript).unwrap();
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 18.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 90.0);
        assert_eq!(snapshot.tertiary.as_ref().unwrap().used_percent, 48.8);
        assert_eq!(snapshot.source, "CLI");
    }

    #[test]
    fn parses_embedded_json_usage() {
        let transcript = "Here is the JSON:\n{\"session_percent\":12,\"week_percent\":34,\"extra_usage_percent\":null}\n";
        let snapshot = parse_usage_snapshot(transcript).unwrap();
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 12.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 34.0);
        assert!(snapshot.tertiary.is_none());
    }

    #[test]
    fn parses_cli_fixture() {
        let transcript = r#"{"session_percent":18,"week_percent":90,"extra_usage_percent":48.8}"#;
        let snapshot = parse_usage_snapshot(transcript).unwrap();
        assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 18.0);
        assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 90.0);
        assert_eq!(snapshot.tertiary.as_ref().unwrap().used_percent, 48.8);
    }
}
