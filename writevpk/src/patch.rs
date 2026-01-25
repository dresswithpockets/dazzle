use byteorder::WriteBytesExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("file '{0}' not found in vpk")]
    NotFound(String),

    #[error("can't patch file that has preload data")]
    HasPreloadData,

    #[error("the input file size ({0} bytes) is larger than the file in the vpk archive ('{1}': {2} bytes)")]
    InputTooBig(u64, String, u64),

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
    fn patch_file(&mut self, path_in_vpk: &str, size: u64, reader: &mut impl Read) -> Result<(), PatchError>;

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

// struct FilePatcher {
//     file: File,
//     start: usize,
//     pos: usize,
//     end: usize,
// }

// impl FilePatcher {
//     pub fn written(&self) -> usize {
//         assert!(self.pos >= self.start);
//         self.pos - self.start
//     }
// }

// impl io::Write for FilePatcher {
//     fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
//         todo!()
//     }

//     fn flush(&mut self) -> io::Result<()> {
//         todo!()
//     }
// }

impl PatchVpkExt for vpk::VPK {
    fn patch_file(&mut self, path_in_vpk: &str, size: u64, reader: &mut impl Read) -> Result<(), PatchError> {
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

        if size > entry_size {
            return Err(PatchError::InputTooBig(size, path_in_vpk.to_string(), entry_size));
        }

        let mut archive_file = OpenOptions::new().write(true).open(archive_path.as_ref())?;
        archive_file.seek(SeekFrom::Start(u64::from(entry.dir_entry.archive_offset)))?;

        let copied = io::copy(reader, &mut archive_file)?;
        if copied != size {
            return Err(PatchError::PartialWrite(copied, entry_size));
        }

        // patched content needs to have the same size as the original, so we pad in 0s to make it fit snuggly.
        if entry_size < copied {
            for _ in entry_size..copied {
                archive_file.write_u8(0)?;
            }
        }

        Ok(())
    }

    fn restore_particles(&mut self, backup_dir: impl AsRef<Path>) -> anyhow::Result<()> {
        let particles_dir = backup_dir.as_ref().join("tf_particles");
        println!("{}", particles_dir.display());
        for entry in fs::read_dir(particles_dir)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }

            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if !file_name.ends_with(".pcf") {
                continue;
            }

            println!("restoring {file_name}");

            //   /particles/example.pcf - the path to the file in the VPK
            let path_in_vpk = "particles/".to_string() + &file_name;

            //   /path/to/backup/particles/example.pcf - the actual on-disk path of the backup particle file
            let path_on_disk = entry.path();

            let size = fs::metadata(&path_on_disk)?.len();
            let mut reader = File::open(&path_on_disk)?;

            if let Err(err) = self.patch_file(&path_in_vpk, size, &mut reader) {
                eprintln!("Error patching particle file '{path_in_vpk}': {err}");
            }
        }
        // let particles_glob = backup_dir.to_str().expect("this should never happen").to_string() + "/particles/*.pcf";
        // let backup_particle_paths = glob(&particles_glob)?
        //     .map(|path| -> anyhow::Result<RelativePathBuf> {
        //         let mut path: &Path = &path?;
        //         if path.is_absolute() {
        //             path = path.strip_prefix(backup_dir)?;
        //         }

        //         Ok(RelativePathBuf::from_path(path)?)
        //     })
        //     .collect::<anyhow::Result<Vec<RelativePathBuf>>>()?;

        // // restore the particles in the misc vpk with our backup, to ensure we're at a clean state
        // for particle_file in backup_particle_paths {
        //     println!("restoring {particle_file}");
        //     // given ./particles/example.pcf, we should map to:

        //     //   /particles/example.pcf - the path to the file in the VPK
        //     let path_in_vpk = particle_file.clone().into_string();

        //     //   /path/to/backup/particles/example.pcf - the actual on-disk path of the backup particle file
        //     let path_on_disk = particle_file.to_path(backup_dir);

        //     let size = fs::metadata(&path_on_disk)?.len();
        //     let mut reader = File::open(&path_on_disk)?;

        //     if let Err(err) = self.patch_file(&path_in_vpk, size, &mut reader) {
        //         eprintln!("Error patching particle file '{particle_file}': {err}");
        //     }
        // }

        Ok(())
    }
}
