use chrono::Local;
use log::LevelFilter;
use std::io::Write;

pub fn init_logger(level: &str, verbose: bool) {
    let filter = if verbose {
        LevelFilter::Debug
    } else {
        level.parse().unwrap_or(LevelFilter::Info)
    };

    env_logger::Builder::new()
        .filter_level(filter)
        .format(|buf, record| {
            writeln!(
                buf,
                "[{} {} {}] {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.target(),
                record.args()
            )
        })
        .init();
}