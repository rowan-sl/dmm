use std::{env, path::PathBuf};

use directories::ProjectDirs;
use lazy_static::lazy_static;

pub static GIT_COMMIT_HASH: &'static str = env!("_GIT_INFO");
pub static PROJECT_NAME: &'static str = clap::crate_name!();
// in git_commit_hash
// pub static PROJECT_VERSION: &'static str = clap::crate_version!();
pub static PROJECT_AUTHORS: &'static str = clap::crate_authors!();

lazy_static! {
    pub static ref DATA_FOLDER: Option<PathBuf> = std::env::var(format!("{}_DATA", PROJECT_NAME))
        .ok()
        .map(PathBuf::from);
    pub static ref CONFIG_FOLDER: Option<PathBuf> =
        std::env::var(format!("{}_CONFIG", PROJECT_NAME))
            .ok()
            .map(PathBuf::from);
    pub static ref LOG_ENV: String = format!("{}_LOGLEVEL", PROJECT_NAME);
    pub static ref LOG_FILE: String = format!("{}.log", env!("CARGO_PKG_NAME"));
}

fn project_directory() -> Option<ProjectDirs> {
    ProjectDirs::from("io", "fawkes", env!("CARGO_PKG_NAME"))
}

pub fn get_data_dir() -> PathBuf {
    let directory = if let Some(s) = DATA_FOLDER.clone() {
        s
    } else if let Some(proj_dirs) = project_directory() {
        proj_dirs.data_local_dir().to_path_buf()
    } else {
        PathBuf::from(".").join(".data")
    };
    directory
}

pub fn get_config_dir() -> PathBuf {
    let directory = if let Some(s) = CONFIG_FOLDER.clone() {
        s
    } else if let Some(proj_dirs) = project_directory() {
        proj_dirs.config_local_dir().to_path_buf()
    } else {
        PathBuf::from(".").join(".config")
    };
    directory
}

pub fn version() -> String {
    let current_exe_path = env::current_exe()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or("unknown".to_string());
    let config_dir_path = get_config_dir().display().to_string();
    let data_dir_path = get_data_dir().display().to_string();

    format!(
        "\
{PROJECT_NAME} {GIT_COMMIT_HASH}

Authors: {PROJECT_AUTHORS}

Config directory: {config_dir_path}
Data directory: {data_dir_path}
exe: {current_exe_path}"
    )
}
