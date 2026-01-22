use std::{borrow::Cow, path::{Path, PathBuf}, str::Utf8Error};

use typed_path::{PlatformPath, Utf8PlatformPath, Utf8PlatformPathBuf};

pub fn to_typed(path: &Path) -> Cow<'_, Utf8PlatformPath> {
    match path.as_os_str().to_string_lossy() {
        Cow::Borrowed(path) => Cow::Borrowed(Utf8PlatformPath::from_bytes_path(PlatformPath::new(path)).unwrap()),
        Cow::Owned(path) => Cow::Owned(Utf8PlatformPathBuf::from(path)),
    }
}

pub fn std_buf_to_typed(path: PathBuf) -> Utf8PlatformPathBuf {
    let string = path.into_os_string().to_string_lossy().into_owned();
    Utf8PlatformPathBuf::from(string)
}

pub fn std_to_typed(path: &Path) -> Result<&Utf8PlatformPath, Utf8Error> {
    Utf8PlatformPath::from_bytes_path(PlatformPath::new(path.as_os_str().as_encoded_bytes()))
}
