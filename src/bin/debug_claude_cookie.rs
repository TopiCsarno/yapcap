/// Debug binary: extract the Claude `sessionKey` cookie from a real browser profile.
/// Run with:
///   cargo run --bin debug_claude_cookie -- brave
///   cargo run --bin debug_claude_cookie -- firefox
///   cargo run --bin debug_claude_cookie -- brave --raw
use yapcap::browser::{load_claude_cookie_chromium, load_claude_cookie_firefox};
use yapcap::config::CursorBrowser;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let raw = args.iter().any(|arg| arg == "--raw");
    let browser = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(|arg| arg.as_str())
        .unwrap_or("brave");

    let browser = match browser {
        "brave" => CursorBrowser::Brave,
        "chrome" => CursorBrowser::Chrome,
        "edge" => CursorBrowser::Edge,
        "firefox" => CursorBrowser::Firefox,
        other => {
            eprintln!("unsupported browser: {other}");
            std::process::exit(2);
        }
    };

    let cookie_db = browser
        .cookie_db_path()
        .expect("could not resolve browser cookie db path");

    let header = match browser.keyring_application() {
        Some(application) => load_claude_cookie_chromium(&cookie_db, application)
            .await
            .expect("failed to load Claude chromium cookie"),
        None => {
            load_claude_cookie_firefox(&cookie_db).expect("failed to load Claude firefox cookie")
        }
    };

    if raw {
        println!("{header}");
        return;
    }

    let preview = if header.len() > 60 {
        format!("{}…  ({} chars total)", &header[..60], header.len())
    } else {
        header
    };
    println!("cookie db: {}", cookie_db.display());
    println!("OK: {preview}");
}
