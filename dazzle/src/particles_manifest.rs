include!(concat!(env!("OUT_DIR"), "/particles_manifest.rs"));

pub fn graphs() -> ordermap::OrderMap<String, Vec<pcf::Pcf>> {
    use byteorder::{LittleEndian, ReadBytesExt};
    use std::io::Read;
    let graph = include_bytes!(concat!(env!("OUT_DIR"), "/particles.graph"));
    let mut reader = bytes::Buf::reader(&graph[..]);
    let mut result = ordermap::OrderMap::new();
    loop {
        match reader.read_u64::<LittleEndian>() {
            Ok(name_len) => {
                let mut name = vec![0u8; name_len as usize];
                reader.read_exact(&mut name).unwrap();
                let name = String::from_utf8(name).unwrap();

                let pcf_len = reader.read_u64::<LittleEndian>().unwrap();
                assert!(pcf_len > 0);
                let mut pcfs = Vec::with_capacity(pcf_len as usize);
                for _idx in 0..pcf_len {
                    let pcf = pcf::decode(&mut reader).unwrap();
                    pcfs.push(pcf);
                }
                result.insert(name, pcfs);
            }
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            _ => panic!("unexpected EOF"),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        os::unix::fs::MetadataExt,
    };

    use pcf::Pcf;

    use crate::particles_manifest::{bins, graphs};

    #[test]
    fn doesnt_panic() {
        let _ = bins();
        let _ = graphs();
    }

    #[test]
    fn equal_to_vanilla_pcfs_with_empty_bodies() {
        let bins = bins();
        for bin in bins {
            let path = format!("vanilla/{}", bin.name());
            let size = fs::metadata(&path).unwrap().size();
            assert_eq!(size, bin.capacity());

            let mut file = File::open_buffered(path).unwrap();
            let pcf = pcf::decode(&mut file).unwrap();

            let empty = Pcf::new_empty_from(&pcf);
            assert_eq!(&empty, bin.as_pcf());
        }
    }
}
