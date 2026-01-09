use std::{borrow::Cow, collections::HashMap, fs, hash::Hash, io, os::unix::fs::MetadataExt};

use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf, Utf8UnixPathBuf};

use crate::paths;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    StripPrefix(#[from] typed_path::StripPrefixError),

    #[error("the source path for a VPK's contents must be a directory")]
    SourceNotADirectory,

    #[error("the destination path for a VPK must be a directory")]
    DestinationNotADirectory,
}

#[derive(Debug, Default)]
struct Entry {
    source_path: Utf8PlatformPathBuf,
    filename: String,
    size: u64,
}

pub fn pack_directory(
    source: &Utf8PlatformPath,
    dest: &Utf8PlatformPath,
    vpk_name: &str,
    split_size: u64,
) -> Result<(), Error> {
    let mut tree = Extensions(HashMap::new());

    if !fs::metadata(source)?.is_dir() {
        return Err(Error::SourceNotADirectory);
    }

    if !fs::metadata(dest)?.is_dir() {
        return Err(Error::DestinationNotADirectory);
    }

    let dir = fs::read_dir(source)?;
    for entry in dir {
        let entry = entry?;
        let size = entry.metadata()?.size();
        let source_path = paths::to_typed(&entry.path()).absolutize()?;
        let extension = source_path.extension().unwrap_or(" ").to_string();
        let filename = source_path.file_stem().unwrap_or(" ").to_string();

        let directory = source_path
            .strip_prefix(source)?
            .parent()
            .map_or(" ".to_string(), |el| el.with_unix_encoding().to_string());

        tree.insert(
            extension,
            directory,
            Entry {
                source_path,
                filename,
                size,
            },
        );
    }

    // TODO: write archive files at {dest}/{vpk_name}_{archive_idx}.vpk
    // TODO: write directory file at {dest}/{vpk_name}_dir.vpk

    Ok(())
}

#[derive(Debug, Default)]
struct Extensions(HashMap<String, Directories>);

#[derive(Debug, Default)]
struct Directories(HashMap<String, Vec<Entry>>);

impl Extensions {
    pub fn insert(&mut self, extension: String, directory: String, entry: Entry) {
        let Some(directories) = self.0.get_mut(&extension) else {
            self.0
                .insert(extension, Directories(HashMap::from([(directory, vec![entry])])));
            return;
        };

        let Some(files) = directories.0.get_mut(&directory) else {
            directories.0.insert(directory, vec![entry]);
            return;
        };

        files.push(entry);
    }
}

// struct SizeChunk {

// }

// impl Iterator for SizeChunk {
//     type Item = Entry;

//     fn next(&mut self) -> Option<Self::Item> {
//         todo!()
//     }
// }

// struct SizeChunkIterator {
//     entries: Vec<Entry>,
//     current_idx: usize,
//     chunk_size: u64,
//     running_size: u64,
// }

// impl Iterator for SizeChunkIterator {
//     type Item = (usize, SizeChunk);

//     fn next(&mut self) -> Option<Self::Item> {
//         let Some(entry) = self.entries.get(self.current_idx) else {
//             return None
//         };

//     }
// }
