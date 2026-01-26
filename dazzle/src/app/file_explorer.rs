use std::{fs, process::Command};

use typed_path::Utf8PlatformPath;

pub(crate) fn open_file_explorer(path: &Utf8PlatformPath) {
    #[cfg(target_os = "windows")]
    fn open(path: &str) {
        Command::new("explorer").arg(path).spawn().unwrap();
    }

    #[cfg(target_os = "linux")]
    fn open(path: &str) {
        Command::new("xdg-open").arg(path).spawn().unwrap();
    }

    let metadata = fs::metadata(path).unwrap();
    if metadata.is_dir() {
        open(path.as_str());
    } else if metadata.is_file() {
        open(path.parent().unwrap().as_str());
    } else {
        panic!("the path provided is not a directory or file, will not open");
    }
}
