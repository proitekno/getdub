pub mod generic;
pub mod media;
pub mod ntfs;

use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsType { Ntfs, Fat32, ExFat, Unknown }

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub filename: String, pub path_rel: String, pub extension: Option<String>,
    pub size: i64, pub created: Option<i64>, pub modified: Option<i64>,
    pub accessed: Option<i64>, pub attributes: Option<i64>, pub fs_specific: Option<FsSpecificData>,
}

#[derive(Debug, Clone)]
pub enum FsSpecificData { Ntfs { frn: u64, parent_frn: u64 } }

#[derive(Debug, Clone)]
pub struct ScanOpts {
    pub skip_hidden: bool, pub skip_system: bool, pub includes: Vec<String>, pub excludes: Vec<String>,
}

pub trait FileSystem {
    fn fs_type(&self) -> FsType;
    fn supports_incremental(&self) -> bool;
    fn scan(&self, root: &str, opts: &ScanOpts) -> Result<Vec<FileEntry>>;
    fn scan_incremental(&self, root: &str, since: Option<u64>, opts: &ScanOpts) -> Result<(Vec<FileEntry>, Option<u64>)>;
    fn current_position(&self, root: &str) -> Result<Option<u64>>;
}

pub fn detect_fs(root: &str) -> Box<dyn FileSystem> {
    let fs_type = detect_volume_type(root);
    match fs_type {
        FsType::Ntfs => Box::new(ntfs::NtfsFileSystem::new()),
        _ => Box::new(generic::GenericFileSystem::new()),
    }
}

#[cfg(windows)]
fn detect_volume_type(root: &str) -> FsType {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
    let root_h = HSTRING::from(root);
    let mut fs_name = [0u16; 64];
    let ok = unsafe { GetVolumeInformationW(&root_h, None, None, None, None, Some(&mut fs_name)) };
    if ok.is_err() { return FsType::Unknown; }
    let len = fs_name.iter().position(|&c| c == 0).unwrap_or(fs_name.len());
    let name = String::from_utf16_lossy(&fs_name[..len]).to_uppercase();
    match name.as_str() {
        "NTFS" => FsType::Ntfs, "FAT32" => FsType::Fat32, "EXFAT" => FsType::ExFat, _ => FsType::Unknown,
    }
}

#[cfg(not(windows))]
fn detect_volume_type(_root: &str) -> FsType { FsType::Unknown }

