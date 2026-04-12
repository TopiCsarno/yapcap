use yapcap::{config::AppConfig, cosmic_app::AppModel, logging};

fn main() -> cosmic::iced::Result {
    let config = match AppConfig::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("failed to load config: {error}");
            AppConfig::default()
        }
    };
    let _logging_guard = match logging::init(&config.log_level) {
        Ok(guard) => Some(guard),
        Err(error) => {
            eprintln!("failed to initialize logging: {error}");
            None
        }
    };
    cosmic::applet::run::<AppModel>(())
}
