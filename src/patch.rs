use anyhow::anyhow;
use glob::glob;
use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};

use relative_path::RelativePathBuf;
use thiserror::Error;
pub use vpk::VPK;

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("file '{0}' not found in vpk")]
    NotFound(String),

    #[error("can't patch file that has preload data")]
    HasPreloadData,

    #[error("the input file's size ({0} bytes) does not match the file in the vpk archive ({1} bytes)")]
    MismatchedSizes(u64, u64),

    #[error("only wrote {0} of the expected {1} bytes")]
    PartialWrite(u64, u64),

    #[error(transparent)]
    IoError(#[from] io::Error),
}

pub trait PrintVpkExt {
    fn print_all_entries(&self);
}

pub trait PatchVpkExt {
    /// Patches data over an existing entry in the vpk's tree.
    ///
    /// The file on disk must have the same size as the file in the VPK, and the file must not have any preload data.
    ///
    /// ## Errors
    ///
    /// Returns [`Err`] if:
    ///
    /// - the file described by `path_in_vpk` does not exist in the vpk
    /// - the file in VPK has a preload data block.
    /// - the file on disk and file in VPK have different sizes
    /// - the function produced no IO error but wasn't able to write the entire file
    /// - there was an IO error when reading the file on disk
    /// - there was an IO error when writing the file on disk
    fn patch_file(&mut self, path_in_vpk: &str, path_on_disk: &Path) -> Result<(), PatchError>;

    /// Searches `backup_dir` PCF files recusively under the `particles` subfolder, and patches them into `self` over
    /// files in the VPK with the same paths relative to `backup_dir`.
    ///
    /// ## Errors
    ///
    /// Returns [`Err`] if:
    ///
    /// - There was an error when searching the `backup_dir`
    /// - There was an error forming a string path for a PCF
    fn restore_particles(&mut self, backup_dir: impl AsRef<Path>) -> anyhow::Result<()>;
}

impl PrintVpkExt for vpk::VPK {
    fn print_all_entries(&self) {
        println!("root_path: {}", self.root_path.display());

        println!("header_length: {}", self.header_length);
        println!("version: {}", self.header.version);
        println!("tree_length: {}", self.header.tree_length);
        println!("signature: {}", self.header.signature);

        if let Some(header_v2) = &self.header_v2 {
            println!("chunk_hashes_length: {}", header_v2.chunk_hashes_length);
            println!("embed_chunk_length: {}", header_v2.embed_chunk_length);
            println!("self_hashes_length: {}", header_v2.self_hashes_length);
            println!("signature_length: {}", header_v2.signature_length);
        }

        println!("{} entries", self.tree.len());

        let mut entries: Vec<_> = self.tree.iter().clone().collect();
        entries.sort_by(|a, b| a.1.dir_entry.archive_index.cmp(&b.1.dir_entry.archive_index));

        for (key, entry) in entries {
            if entry.dir_entry.archive_index == 0 {
                println!("entry in {} at '{key}'", entry.dir_entry.archive_index);
            }
            // if let Some(archive_dir) = &entry.archive_path {
            //     println!("- archive_dir: '{}'", archive_dir.display());
            // }
            // println!("- preload data len: {}", entry.preload_data.len());
        }
    }
}

impl PatchVpkExt for vpk::VPK {
    fn patch_file(&mut self, path_in_vpk: &str, path_on_disk: &Path) -> Result<(), PatchError> {
        let entry = self
            .tree
            .get(path_in_vpk)
            .ok_or_else(|| PatchError::NotFound(path_in_vpk.to_string()))?;

        if entry.dir_entry.preload_length > 0 {
            return Err(PatchError::HasPreloadData);
        }

        let Some(archive_path) = &entry.archive_path else {
            return Err(PatchError::HasPreloadData);
        };

        // TODO: what about preload_length? does patch_file need to ever handle preloaded files?
        let entry_size = u64::from(entry.dir_entry.file_length);
        let new_file_size = path_on_disk.symlink_metadata()?.len();

        if entry_size != new_file_size {
            return Err(PatchError::MismatchedSizes(new_file_size, entry_size));
        }

        let new_file = File::open(path_on_disk)?;
        let mut new_file = BufReader::new(new_file);

        let mut archive_file = OpenOptions::new().write(true).open(archive_path.as_ref())?;
        archive_file.seek(SeekFrom::Start(u64::from(entry.dir_entry.archive_offset)))?;

        // TODO!: this is probably _inserting_ new data, not _replacing_ existing data

        let copied = io::copy(&mut new_file, &mut archive_file)?;
        if copied != entry_size {
            return Err(PatchError::PartialWrite(copied, entry_size));
        }

        Ok(())
    }

    fn restore_particles(&mut self, backup_dir: impl AsRef<Path>) -> anyhow::Result<()> {
        let backup_dir = backup_dir.as_ref();
        let particles_glob = backup_dir.to_str().expect("this should never happen").to_string() + "/particles/**/*.pcf";
        let backup_particle_paths = glob(&particles_glob)?
            .map(|path| -> anyhow::Result<RelativePathBuf> {
                let mut path: &Path = &path?;
                if path.is_absolute() {
                    path = path.strip_prefix(backup_dir)?;
                }

                Ok(RelativePathBuf::from_path(path)?)
            })
            .collect::<anyhow::Result<Vec<RelativePathBuf>>>()?;

        // restore the particles in the misc vpk with our backup, to ensure we're at a clean state
        for particle_file in backup_particle_paths {
            // given ./particles/example.pcf, we should map to:

            //   /particles/example.pcf - the path to the file in the VPK
            let path_in_vpk = particle_file.clone().into_string();

            //   /path/to/backup/particles/example.pcf - the actual on-disk path of the backup particle file
            let path_on_disk = particle_file.to_path(backup_dir);

            if let Err(err) = self.patch_file(&path_in_vpk, &path_on_disk) {
                eprintln!("Error patching particle file '{particle_file}': {err}");
            }
        }

        Ok(())
    }
}
