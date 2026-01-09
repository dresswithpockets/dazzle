use anyhow::anyhow;
use copy_dir::copy_dir;
use glob::glob;
use std::{
    collections::HashMap, fs::{self, File, OpenOptions}, io::{self, BufWriter, Read}, path::{Path, PathBuf}
};
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};
use vpk::VPK;

use crate::paths::{self, std_to_typed};

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

    /// A set of absolute VTF paths, provided by the addon
    pub texture_files: HashMap<String, Utf8PlatformPathBuf>,

    /// A map of "{addon}/materials/"-relative VMT paths to decoded VMTs, provided by the addon
    pub relative_material_files: HashMap<String, Material>,

    /// A map of absolute PCF paths to decoded PCFs, provided by the addon
    pub particle_files: HashMap<Utf8PlatformPathBuf, pcf::Pcf>,
}

#[derive(Debug, Clone)]
pub struct Material {
    /// the path to this material, relative to `{path_to_game}/materials/`
    pub relative_path: Utf8PlatformPathBuf,

    /// $basetexture value if specified in the material
    pub base_texture: Option<String>,

    /// $detail value if specified in the material
    pub detail: Option<String>,

    /// $ramptexture value if specified in the material
    pub ramp_texture: Option<String>,

    /// $normalmap value if specified in the material
    pub normal_map: Option<String>,

    /// $normalmap2 value if specified in the material
    pub normal_map_2: Option<String>,
}

#[derive(Debug)]
pub struct Extracted {
    source_path: Utf8PlatformPathBuf,
    content_path: Utf8PlatformPathBuf,
}

impl Extracted {
    fn get_material_files(materials_path: &Utf8PlatformPath) -> anyhow::Result<HashMap<String, Material>>  {
        fn value_to_texture_name(cow: &str) -> String {
            let owned = cow.to_owned();
            if owned.eq_ignore_ascii_case(".vtf") {
                owned
            } else {
                owned + ".vtf"
            }
        }

        let mut relative_material_files = HashMap::new();
        for path in glob(&format!("{materials_path}/**/*.vmt"))? {
            let path = path?;
            let path = paths::to_typed(&path).absolutize()?;
            let relative_path = path.strip_prefix(materials_path)?.to_owned();

            let mut vmt_buf = String::new();
            File::open_buffered(&path)?
                .read_to_string(&mut vmt_buf)?;

            let root = keyvalues_parser::parse(&vmt_buf)?;

            // vtf parameters will always be keys on the first value
            let keyvalues_parser::Value::Obj(values) = root.value else {
                return Err(anyhow!("malformed VMT '{}'", &path));
            };

            let mut material = Material {
                relative_path: relative_path.clone(),
                base_texture: None,
                detail: None,
                ramp_texture: None,
                normal_map: None,
                normal_map_2: None,
            };

            for (key, values) in values.iter() {
                let Some(keyvalues_parser::Value::Str(value)) = values.first() else {
                    continue;
                };

                match key as &str {
                    "$basetexture" => material.base_texture = Some(value_to_texture_name(value)),
                    "$detail" => material.detail = Some(value_to_texture_name(value)),
                    "$ramptexture" => material.ramp_texture = Some(value_to_texture_name(value)),
                    "$normalmap" => material.normal_map = Some(value_to_texture_name(value)),
                    "$normalmap2" => material.normal_map_2 = Some(value_to_texture_name(value)),
                    _ => {},
                }
            }

            relative_material_files.insert(relative_path.into_string(), material.clone());
        }

        Ok(relative_material_files)
    }

    /// parses the contents of an extracted addon into an [`Addon`].
    ///
    /// # Errors
    ///
    /// May return [`Err`] if:
    ///
    /// - iterating over extracted files fails
    /// - some [`std::io::Error`] when opening or reading files
    /// - the addon contains invalid or inoperable parts, such as a corrupted PCF.
    pub fn parse_content(self) -> anyhow::Result<Addon> {
        let mut particle_files = HashMap::new();
        let particles_path = self.content_path.join_checked("particles")?;
        for path in glob(&format!("{particles_path}/*.pcf"))? {
            let path = path?;
            let path = paths::to_typed(&path);

            let mut file = File::open_buffered(path.as_ref())?;
            let pcf = pcf::Pcf::decode(&mut file)?;
            particle_files.insert(path.into_owned(), pcf);
        }
        
        let materials_path = self.content_path.join_checked("materials")?;
        let relative_material_files = Self::get_material_files(&materials_path)?;

        let mut texture_files = HashMap::new();
        for path in glob(&format!("{}/**/*.vtf", &materials_path))? {
            let path = path?;
            let path = paths::to_typed(&path).absolutize()?;
            let relative_path = path.strip_prefix(&materials_path)?;
            texture_files.insert(relative_path.to_string(), path);
        }

        Ok(Addon {
            content_path: self.content_path,
            source_path: self.source_path,
            texture_files,
            relative_material_files,
            particle_files,
        })
    }
}

#[derive(Debug)]
/// A collection of all sources read with [`Sources::read_dir`].
pub struct Sources {
    pub sources: Box<[Source]>,
    pub failures: Box<[(PathBuf, Error)]>,
}

impl Sources {
    /// Searches `addons_dir` for addon sources, and produces a [`Vec`] of [`Source`].
    ///
    /// ## Errors
    ///
    /// See [`fs::read_dir`] for potential terminal errors. Some failures won't result in [Err]: The resulting
    /// [`Sources::failures`] will contain information about each entry in `addons_dir` that produced an error.
    pub fn read_dir(addons_dir: impl AsRef<Path>) -> Result<Sources, Error> {
        let mut sources = Vec::new();
        let mut failures = Vec::new();
        for entry in addons_dir.as_ref().read_dir()? {
            let entry = entry?;
            let path = entry.path();
            match Source::from_path(&path) {
                Ok(source) => sources.push(source),
                Err(err) => failures.push((path, err)),
            }
        }

        Ok(Sources {
            sources: sources.into_boxed_slice(),
            failures: failures.into_boxed_slice(),
        })
        // let addons_glob = addons_dir.as_ref().join("*");
        // let addons_glob = addons_glob.to_str().expect("this should never happen");
        // glob(addons_glob)?.map(|path| Source::from_path(&path?)).collect()
    }
}

#[derive(Debug)]
/// An addon source. Points to a folder or supported archive file like a VPK.
///
/// See [`Sources::read_dir`] to read sources from a directory.
pub enum Source {
    Folder(Utf8PlatformPathBuf),
    // TODO: support .zip, .tar, .tar.br, .tar.bz2, .tar.gz, .tar.lzma, etc
    Vpk(Utf8PlatformPathBuf),
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("unsupported addon type at '{0}'")]
    UnsupportedAddonType(Utf8PlatformPathBuf),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),
}

impl Source {
    /// Evaluates the `source` path to determine the [`Source`] type.
    ///
    /// Can be one of:
    ///
    /// - folder
    /// - vpk
    ///
    /// ## Errors
    ///
    /// - [`Error::UnsupportedAddonType`] if the path points to an addon that doesn't exist, or is not one of the supported types.
    pub fn from_path(source: &std::path::Path) -> Result<Source, Error> {
        let metadata = fs::metadata(source)?;
        let source = std_to_typed(source)?;
        if metadata.is_dir() {
            Ok(Source::Folder(source.to_path_buf()))
        } else if metadata.is_file()
            && let Some(extension) = source.extension()
            && extension.eq_ignore_ascii_case("vpk")
        {
            Ok(Source::Vpk(source.to_path_buf()))
        } else {
            Err(Error::UnsupportedAddonType(source.to_path_buf()))
        }
    }

    /// Copies the contents of the source into a subfolder under [`parent`]. The subfolder will be named after the name
    /// of the source.
    ///
    /// For example, if the Source points to a file `/path/to/addon.vpk` then the subfolder will be `{parent}/addon.vpk/`.
    ///
    /// Returns [`Extracted`] pointing to the extracted contents.
    ///
    /// ## Errors
    ///
    /// Errors if:
    ///
    /// - the source is missing a file or directory name
    /// - a valid subfolder path couldn't be formed
    /// - `parent` doesn't exist
    /// - the destination subfolder already exists
    /// - there was an error extracting the source's contents, e.g. not enough permissions to write to the folder
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
