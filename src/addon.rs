use anyhow::anyhow;
use copy_dir::copy_dir;
use glob::glob;
use std::{
    collections::HashMap, fs::{self, File, OpenOptions}, io::{self, BufWriter}, path::Path
};
use typed_path::{PlatformPath, Utf8PlatformPath, Utf8PlatformPathBuf};
use vpk::VPK;

#[derive(Debug)]
pub struct Info {
    pub name: String,
    pub mod_type: String,
    pub description: String,
    pub author: String,
}

#[derive(Debug)]
pub struct Addon {
    // TODO: pub info: Info,

    /// the path where all content has been extracted or copied
    pub content_path: Utf8PlatformPathBuf,

    /// the path to the source file (vpk) or folder of the addon content
    pub source_path: Utf8PlatformPathBuf,

    /// A list of PCF names provided by the addon
    pub particle_files: HashMap<Utf8PlatformPathBuf, pcf::Pcf>,
}

#[derive(Debug)]
pub struct Extracted {
    source_path: Utf8PlatformPathBuf,
    content_path: Utf8PlatformPathBuf,
}

impl Extracted {
    pub fn parse_content(self) -> anyhow::Result<Addon> {
        let mut particle_files = HashMap::new();
        let particles_path = self.content_path.join_checked("particles")?;
        for path in glob(&format!("{particles_path}/*.pcf"))? {
            let path = path?;
            let path = Utf8PlatformPath::from_bytes_path(PlatformPath::new(path.as_os_str().as_encoded_bytes()))?;

            let mut file = File::open_buffered(path)?;
            let pcf = pcf::Pcf::decode(&mut file)?;
            particle_files.insert(path.to_path_buf(), pcf);
        }

        Ok(Addon {
            content_path: self.content_path,
            source_path: self.source_path,
            particle_files,
        })
    }
}

#[derive(Debug)]
pub enum Source {
    Folder(Utf8PlatformPathBuf),
    // TODO: support .zip, .tar, .tar.br, .tar.bz2, .tar.gz, .tar.lzma, etc
    Vpk(Utf8PlatformPathBuf),
}

impl Source {
    pub fn get_addon_sources(addons_dir: impl AsRef<Path>) -> anyhow::Result<Vec<Source>> {
        let mods_glob = addons_dir
            .as_ref()
            .to_str()
            .expect("this should never happen")
            .to_string()
            + "/mods/*.vpk";
        glob(&mods_glob)?.map(|path| Source::from_path_owned(&path?)).collect()
    }

    pub fn from_path_owned(source: &std::path::Path) -> anyhow::Result<Source> {
        let metadata = fs::metadata(source)?;
        let source = Utf8PlatformPath::from_bytes_path(PlatformPath::new(source.as_os_str().as_encoded_bytes()))?;
        if metadata.is_dir() {
            Ok(Source::Folder(source.to_path_buf()))
        } else if metadata.is_file()
            && let Some(extension) = source.extension()
            && extension.eq_ignore_ascii_case("vpk")
        {
            Ok(Source::Vpk(source.to_path_buf()))
        } else {
            Err(anyhow!("unsupported addon type at {source}"))
        }
    }

    pub fn extract_as_subfolder_in(&self, parent: &Utf8PlatformPath) -> anyhow::Result<Extracted> {
        let source_path = match self {
            Source::Folder(source_path) | Source::Vpk(source_path) => source_path,
        };

        let last_part = source_path
            .file_name()
            .ok_or(anyhow!("couldn't get last component from addon path: {source_path}"))?;
        let destination = parent.join_checked(last_part)?;

        if !fs::exists(parent)? {
            return Err(anyhow!(
                "the addon extraction parent '{parent}' doesn't exist. this should never happen."
            ));
        }

        if fs::exists(&destination)? {
            return Err(anyhow!(
                "the addon extraction destination '{destination}' already exists. this should never happen."
            ));
        }

        match self {
            Source::Folder(source_path) => {
                let errors = copy_dir(source_path, &destination)?;
                if !errors.is_empty() {
                    return Err(anyhow!(""));
                }
            }
            Source::Vpk(source_path) => Self::extract_vpk(source_path, &destination)?,
        }

        Ok(Extracted {
            source_path: source_path.clone(),
            content_path: destination,
        })
    }

    /// Extracts the entire file tree from a vpk at `source_vpk` to a target directory `to_dir`.
    fn extract_vpk(source_vpk: impl AsRef<Path>, to_dir: impl AsRef<Path>) -> anyhow::Result<()> {
        let vpk = VPK::read(&source_vpk)?;

        // TODO: make vpk extraction asynchronous/threaded
        for (entry_path, entry) in vpk.tree {
            let mut file_in_vpk = entry.reader()?;

            let entry_path = entry_path.trim_prefix('/');
            let file_path = to_dir.as_ref().join(entry_path);

            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let extracted_file = OpenOptions::new().write(true).create_new(true).open(&file_path)?;
            let mut extracted_file = BufWriter::new(extracted_file);

            let entry_size = u64::from(entry.dir_entry.file_length) + u64::from(entry.dir_entry.preload_length);
            let copied = io::copy(&mut file_in_vpk, &mut extracted_file)?;
            if copied != entry_size {
                return Err(anyhow!(
                    "expected to copy {entry_size}, instead copied {copied}, when copying {}/{entry_path} to {}",
                    source_vpk.as_ref().display(),
                    file_path.display()
                ));
            }
        }

        Ok(())
    }
}
