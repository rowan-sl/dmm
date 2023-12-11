use std::env;

use const_cmp::const_eq;
use lazy_static::lazy_static;
use static_assertions::const_assert;

pub static GIT_COMMIT_HASH: &'static str = env!("_GIT_INFO");
pub static PROJECT_NAME: &'static str = clap::crate_name!();
// in git_commit_hash
// pub static PROJECT_VERSION: &'static str = clap::crate_version!();
pub static PROJECT_AUTHORS: &'static str = clap::crate_authors!();

/// Codename associated with the current project version
pub static PROJECT_VERSION_CODENAME: &'static str = "Margarine";
const_assert!(const_eq!(clap::crate_version!(), "0.2.0"));

lazy_static! {
    pub static ref LOG_ENV: String = format!("{}_LOGLEVEL", PROJECT_NAME);
    pub static ref LOG_FILE: String = format!("{}.log", env!("CARGO_PKG_NAME"));
}

pub fn version() -> String {
    let current_exe_path = env::current_exe()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or("unknown".to_string());
    let debug_info = if cfg!(debug_assertions) {
        "[debug build]"
    } else {
        ""
    };

    format!(
        "\
{PROJECT_NAME} {GIT_COMMIT_HASH} ({PROJECT_VERSION_CODENAME}) {debug_info}
Authors: {PROJECT_AUTHORS}
Repository: https://git.fawkes.io/mtnash/dmm

exe: {current_exe_path}"
    )
}
