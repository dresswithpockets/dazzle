use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};

mod serde_path_string {
    use serde::{Deserializer, Serializer, de::Visitor};
    use typed_path::{Utf8PlatformPathBuf, Utf8TypedPath, Utf8UnixPathBuf, Utf8WindowsPathBuf};

    pub(crate) fn serialize<S>(path: &Utf8PlatformPathBuf, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ser.serialize_str(path.as_str())
    }

    pub(crate) fn deserialize<'de, D>(de: D) -> Result<Utf8PlatformPathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        de.deserialize_string(PathVisitor)
    }

    struct PathVisitor;
    impl Visitor<'_> for PathVisitor {
        type Value = Utf8PlatformPathBuf;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string containing a valid unix path")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if v.is_empty() {
                Ok(Utf8PlatformPathBuf::from(v))
            } else {
                match Utf8TypedPath::derive(v) {
                    Utf8TypedPath::Unix(path) => Ok(path.with_platform_encoding()),
                    Utf8TypedPath::Windows(path) => Ok(path.with_platform_encoding()),
                }
            }
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if v.is_empty() {
                Ok(Utf8PlatformPathBuf::from(v))
            } else {
                match Utf8TypedPath::derive(&v) {
                    Utf8TypedPath::Unix(_) => Ok(Utf8UnixPathBuf::from(v).with_platform_encoding()),
                    Utf8TypedPath::Windows(_) => Ok(Utf8WindowsPathBuf::from(v).with_platform_encoding()),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, with = "serde_path_string")]
    pub tf_dir: Utf8PlatformPathBuf,

    #[serde(default)]
    pub addons: HashMap<String, AddonConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AddonConfig {
    #[serde(default = "AddonConfig::default_enabled")]
    pub enabled: bool,

    #[serde(default = "AddonConfig::default_order")]
    pub order: usize,
}

impl Default for AddonConfig {
    fn default() -> Self {
        AddonConfig::DEFAULT
    }
}

impl AddonConfig {
    const DEFAULT: AddonConfig = AddonConfig {
        enabled: true,
        order: usize::MAX,
    };

    fn default_enabled() -> bool {
        true
    }

    fn default_order() -> usize {
        usize::MAX
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("couldn't read or write the app config, due to an IO error")]
    Io(#[from] io::Error),

    #[error("couldn't encode the app config")]
    ToString(#[from] toml::ser::Error),

    #[error("couldn't parse the app config")]
    Parse(#[from] toml::de::Error),
}

pub fn create_or_read_config(path: &Utf8PlatformPath) -> Result<Config, Error> {
    let _ = fs::create_dir_all(path.parent().unwrap());
    let mut file = OpenOptions::new().create(true).append(true).read(true).open(path)?;
    let mut config = String::new();
    file.read_to_string(&mut config)?;
    Ok(toml::from_str(&config)?)
}

pub fn write_config(path: &Utf8PlatformPath, config: &Config) -> Result<(), Error> {
    let _ = fs::create_dir_all(path.parent().unwrap());
    let mut file = OpenOptions::new().create(true).truncate(true).write(true).open(path)?;
    let config = toml::to_string_pretty(&config)?;
    file.write_all(config.as_bytes())?;
    file.flush()?;
    Ok(())
}
