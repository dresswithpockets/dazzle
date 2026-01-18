use std::{collections::HashMap, ffi::CString};

use typed_path::Utf8PlatformPathBuf;
use vpk::VPK;

#[derive(Debug)]
pub struct App {
    pub addons_dir: Utf8PlatformPathBuf,
    pub extracted_content_dir: Utf8PlatformPathBuf,
    pub backup_dir: Utf8PlatformPathBuf,
    pub working_vpk_dir: Utf8PlatformPathBuf,

    pub vanilla_pcf_paths: Vec<Utf8PlatformPathBuf>,
    pub vanilla_pcf_to_systems: HashMap<String, Vec<CString>>,
    pub vanilla_system_to_pcf: HashMap<CString, String>,

    pub tf_misc_vpk: VPK,
    pub tf_custom_dir: Utf8PlatformPathBuf,
}
