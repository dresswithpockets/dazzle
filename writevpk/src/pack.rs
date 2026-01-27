use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Read, Seek, Write},
    os::unix::fs::MetadataExt,
};

use buf_read_write::BufStream;
use byteorder::{LittleEndian, WriteBytesExt};
use md5::{Digest, Md5};
use thiserror::Error;
use typed_path::{Utf8PlatformPath, Utf8PlatformPathBuf};

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

    #[error("failed to remove the 0th archive, due to an IO error")]
    CantRemoveArchive0(io::Error),

    #[error("failed to rename the dir-index VPK archive, due to an IO error")]
    CantRenameDirArchive(io::Error),

    #[error("failed to create and open a new vpk, due to an IO error")]
    CantOpenVpk(io::Error),

    #[error("failed to create and open a new dir-index vpk, due to an IO error")]
    CantOpenDirVpk(io::Error),

    #[error("failed to open an entry file '{0}', due to an IO error")]
    CantOpenEntrySource(typed_path::Utf8PathBuf<typed_path::Utf8PlatformEncoding>, io::Error),
}

#[derive(Debug, Default)]
struct Entry {
    source_path: Utf8PlatformPathBuf,
    filename: String,
    size: u32,
}

#[derive(Debug, Default)]
struct EntryInfo {
    filename: String,
    archive_idx: u16,
    offset: u32,
    size: u32,
    crc: u32,
}

pub fn pack_directory(
    source: &Utf8PlatformPath,
    dest: &Utf8PlatformPath,
    vpk_name: &str,
    split_size: u32,
) -> Result<(), Error> {
    if !fs::metadata(source)?.is_dir() {
        return Err(Error::SourceNotADirectory);
    }

    if !fs::metadata(dest)?.is_dir() {
        return Err(Error::DestinationNotADirectory);
    }

    let tree = get_vpk_tree(source)?;
    let (last_archive_path, last_archive_idx, tree) = write_tree(tree, dest, vpk_name, split_size)?;

    let vpk_path = dest.join(format!("{vpk_name}_dir.vpk"));
    write_index_archive(&last_archive_path, last_archive_idx, tree, &vpk_path)?;

    if last_archive_idx == 0 {
        // we copied our 0th archive into _dir, so we need to drop the "_dir" and remove the 0th archive.
        fs::remove_file(last_archive_path).map_err(Error::CantRemoveArchive0)?;

        fs::rename(vpk_path, dest.join(vpk_name).with_extension("vpk")).map_err(Error::CantRenameDirArchive)?;
    }

    Ok(())
}

fn write_index_archive(
    last_archive_path: &Utf8PlatformPath,
    last_archive_idx: u16,
    tree: VpkTree<EntryInfo>,
    vpk_path: &Utf8PlatformPath,
) -> Result<(), Error> {
    let mut stream = BufStream::new(
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(vpk_path)
            .map_err(Error::CantOpenDirVpk)?,
    );

    const VPK_SIGNATURE: u32 = 0x55AA1234;
    const VPK_VERSION: u32 = 2;
    const VPK_CHUNK_HASHES_LENGTH: u32 = 0;
    const VPK_SELF_HASHES_LENGTH: u32 = 48;
    const VPK_SIGNATURE_LENGTH: u32 = 0;

    stream.write_u32::<LittleEndian>(VPK_SIGNATURE)?;
    stream.write_u32::<LittleEndian>(VPK_VERSION)?;

    let tree_size_offset = stream.stream_position()?;
    stream.write_u32::<LittleEndian>(0)?;

    let embed_chunk_offset = stream.stream_position()?;
    stream.write_u32::<LittleEndian>(0)?;

    stream.write_u32::<LittleEndian>(VPK_CHUNK_HASHES_LENGTH)?;
    stream.write_u32::<LittleEndian>(VPK_SELF_HASHES_LENGTH)?;
    stream.write_u32::<LittleEndian>(VPK_SIGNATURE_LENGTH)?;

    let tree_start = stream.stream_position()?;
    for (extension, directories) in tree.0 {
        _ = stream.write(extension.as_bytes())?;
        stream.write_u8(0)?;

        for (dir_path, entries) in directories.0 {
            _ = stream.write(dir_path.as_bytes())?;
            stream.write_u8(0)?;

            for entry in entries {
                _ = stream.write(entry.filename.as_bytes())?;
                stream.write_u8(0)?;

                stream.write_u32::<LittleEndian>(entry.crc)?;
                // this impl doesnt support writing preload data, so there is always 0
                stream.write_u16::<LittleEndian>(0)?;
                if last_archive_idx == 0 {
                    stream.write_u16::<LittleEndian>(u16::MAX >> 1)?;
                } else {
                    stream.write_u16::<LittleEndian>(entry.archive_idx)?;
                }
                stream.write_u32::<LittleEndian>(entry.offset)?;
                stream.write_u32::<LittleEndian>(entry.size)?;
                stream.write_u16::<LittleEndian>(0xFFFF)?;
            }

            stream.write_u8(0)?;
        }

        stream.write_u8(0)?;
    }

    stream.write_u8(0)?;

    let tree_size = (stream.stream_position()? - tree_start) as u32;

    let embed_chunk_size = if last_archive_idx == 0 {
        let mut last_archive_file = File::open_buffered(last_archive_path)?;
        io::copy(&mut last_archive_file, &mut stream)? as u32
    } else {
        0
    };

    stream.flush()?;

    let chunk_hashes = Md5::new().finalize();
    let tree_hash = {
        stream.seek(io::SeekFrom::Start(tree_start))?;
        let mut tree_reader = Read::by_ref(&mut stream).take(tree_size as u64);

        let mut tree_hasher = Md5::new();
        io::copy(&mut tree_reader, &mut tree_hasher)?;

        tree_hasher.finalize()
    };

    let file_hash = {
        stream.seek(io::SeekFrom::Start(0))?;
        let mut header_reader = Read::by_ref(&mut stream).take(tree_start);

        let mut file_hasher = Md5::new();
        io::copy(&mut header_reader, &mut file_hasher)?;
        file_hasher.write_all(&tree_hash)?;
        file_hasher.write_all(&chunk_hashes)?;
        file_hasher.finalize()
    };

    stream.seek(io::SeekFrom::Start(tree_size_offset))?;
    stream.write_u32::<LittleEndian>(tree_size as u32)?;

    stream.seek(io::SeekFrom::Start(embed_chunk_offset))?;
    stream.write_u32::<LittleEndian>(embed_chunk_size)?;

    stream.seek(io::SeekFrom::End(0))?;
    stream.write_all(&tree_hash)?;
    stream.write_all(&chunk_hashes)?;
    stream.write_all(&file_hash)?;

    stream.flush()?;

    Ok(())
}

fn write_tree(
    tree: VpkTree<Entry>,
    dest: &Utf8PlatformPath,
    vpk_name: &str,
    split_size: u32,
) -> Result<(Utf8PlatformPathBuf, u16, VpkTree<EntryInfo>), Error> {
    let mut archive_path = dest.join(format!("{vpk_name}_000.vpk"));
    let mut archive_file = BufWriter::new(
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&archive_path)
            .map_err(Error::CantOpenVpk)?,
    );

    let mut current_archive_idx = 0;
    let mut current_size = 0;

    let mut written_tree = VpkTree(HashMap::new());
    for (extension, directories) in tree.0 {
        for (dir_path, entries) in directories.0 {
            for entry in entries {
                if current_size > 0 && current_size + entry.size > split_size {
                    archive_file.flush()?;
                    current_archive_idx += 1;
                    current_size = 0;

                    archive_path = dest.with_file_name(format!("{vpk_name}_{current_archive_idx:03}.vpk"));
                    archive_file = BufWriter::new(
                        OpenOptions::new()
                            .create(true)
                            .truncate(true)
                            .write(true)
                            .open(&archive_path)
                            .map_err(Error::CantOpenVpk)?,
                    );
                }

                let results =
                    fs::read(&entry.source_path).map_err(|err| Error::CantOpenEntrySource(entry.source_path, err))?;
                let checksum = crc32fast::hash(&results);

                _ = archive_file.write(&results)?;

                written_tree.insert(
                    &extension,
                    &dir_path,
                    EntryInfo {
                        filename: entry.filename,
                        archive_idx: current_archive_idx,
                        offset: current_size,
                        size: entry.size,
                        crc: checksum,
                    },
                );

                current_size += results.len() as u32;
            }
        }
    }

    archive_file.flush()?;

    Ok((archive_path, current_archive_idx, written_tree))
}

fn get_vpk_tree(source: &Utf8PlatformPath) -> Result<VpkTree<Entry>, Error> {
    fn visit(source: &Utf8PlatformPath, dir: &Utf8PlatformPath, tree: &mut VpkTree<Entry>) -> Result<(), Error> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let source_path = paths::to_typed(&entry.path()).absolutize()?;

            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                visit(source, &source_path, tree)?;
                continue;
            }

            let size = metadata.size() as u32;
            let extension = source_path.extension().unwrap_or(" ").to_string();
            let filename = source_path.file_stem().unwrap_or(" ").to_string();

            let directory = source_path
                .strip_prefix(source)?
                .parent()
                .map_or(" ".to_string(), |el| el.with_unix_encoding().to_string());

            tree.insert(
                &extension,
                &directory,
                Entry {
                    source_path,
                    filename,
                    size,
                },
            );
        }

        Ok(())
    }

    let mut tree = VpkTree(HashMap::new());
    visit(source, source, &mut tree)?;

    Ok(tree)
}

#[derive(Debug, Default)]
struct VpkTree<T: Sized>(HashMap<String, Directories<T>>);

#[derive(Debug, Default)]
struct Directories<T: Sized>(HashMap<String, Vec<T>>);

impl<T: Sized> VpkTree<T> {
    pub fn insert(&mut self, extension: &str, directory: &str, entry: T) {
        let Some(directories) = self.0.get_mut(extension) else {
            self.0.insert(
                extension.to_string(),
                Directories(HashMap::from([(directory.to_string(), vec![entry])])),
            );
            return;
        };

        let Some(files) = directories.0.get_mut(directory) else {
            directories.0.insert(directory.to_string(), vec![entry]);
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
