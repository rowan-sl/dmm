use std::path::PathBuf;

use color_eyre::eyre::Result;
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    self, fmt::writer::BoxMakeWriter, prelude::__tracing_subscriber_SubscriberExt,
    util::SubscriberInitExt, Layer,
};

use crate::project_meta::{get_data_dir, LOG_ENV};

pub fn initialize_logging(path: Option<PathBuf>) -> Result<()> {
    let directory = get_data_dir();
    std::fs::create_dir_all(directory.clone())?;
    std::env::set_var(
        "RUST_LOG",
        std::env::var("RUST_LOG")
            .or_else(|_| std::env::var(LOG_ENV.clone()))
            .unwrap_or_else(|_| format!("{}=info", env!("CARGO_CRATE_NAME"))),
    );
    let file_subscriber = tracing_subscriber::fmt::layer()
        .with_file(true)
        .with_line_number(true)
        .with_writer(
            path.as_ref()
                .map(|x| Ok::<_, std::io::Error>(BoxMakeWriter::new(std::fs::File::create(x)?)))
                .unwrap_or(Ok(BoxMakeWriter::new(std::io::stdout)))?,
        )
        .with_target(path.is_some())
        .with_ansi(false)
        .with_filter(tracing_subscriber::filter::EnvFilter::from_default_env());
    tracing_subscriber::registry()
        .with(file_subscriber)
        .with(ErrorLayer::default())
        .init();
    Ok(())
}

/// Similar to the `std::dbg!` macro, but generates `tracing` events rather
/// than printing to stdout.
///
/// By default, the verbosity level for the generated events is `DEBUG`, but
/// this can be customized.
#[macro_export]
macro_rules! trace_dbg {
    (target: $target:expr, level: $level:expr, $ex:expr) => {{
        match $ex {
            value => {
                tracing::event!(target: $target, $level, ?value, stringify!($ex));
                value
            }
        }
    }};
    (level: $level:expr, $ex:expr) => {
        trace_dbg!(target: module_path!(), level: $level, $ex)
    };
    (target: $target:expr, $ex:expr) => {
        trace_dbg!(target: $target, level: tracing::Level::DEBUG, $ex)
    };
    ($ex:expr) => {
        trace_dbg!(level: tracing::Level::DEBUG, $ex)
    };
}
