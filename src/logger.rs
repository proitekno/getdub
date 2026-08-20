use log::LevelFilter;

pub fn init_logger(level: &str, verbose: bool) {
    let filter = if verbose {
        LevelFilter::Debug
    } else {
        level.parse().unwrap_or(LevelFilter::Info)
    };

    env_logger::Builder::new()
        .filter_level(filter)
        .format_timestamp_secs()
        .init();
}

