/// Debug binary: extract the Cursor session cookie from the real Firefox profile.
/// Run with:  cargo run --bin debug_firefox
use yapcap::browser::load_cursor_cookie_firefox;
use yapcap::config::CursorBrowser;

fn main() {
    let cookie_db = CursorBrowser::Firefox
        .cookie_db_path()
        .expect("could not resolve Firefox cookie db path");

    println!("cookie db: {}", cookie_db.display());

    match load_cursor_cookie_firefox(&cookie_db) {
        Ok(header) => {
            // Print only the first 60 chars so the token isn't fully exposed in terminal history.
            let preview = if header.len() > 60 {
                format!("{}…  ({} chars total)", &header[..60], header.len())
            } else {
                header.clone()
            };
            println!("OK: {preview}");
        }
        Err(err) => {
            eprintln!("ERR: {err}");
            std::process::exit(1);
        }
    }
}
