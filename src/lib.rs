use std::path::PathBuf;

pub fn get_base_dir() -> PathBuf {
    std::env::current_exe().unwrap().canonicalize().unwrap()
}
