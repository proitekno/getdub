use super::generic::GenericFileSystem;
use super::media::{self, MediaType};
use super::{FileEntry, FsSpecificData, FsType, FileSystem, ScanOpts};
use anyhow::Result;
use log::{info, warn};
use std::path::Path;

#[cfg(windows)]
use crate::config::win::FSCTL_QUERY_USN_JOURNAL;

pub struct NtfsFileSystem;

impl NtfsFileSystem {
    pub fn new() -> Self { NtfsFileSystem }

    #[cfg(windows)]
    pub fn query_usn_journal(root: &str) -> Result<Option<u64>> {
        use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, INVALID_HANDLE_VALUE};
        use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
        use windows::Win32::System::IO::DeviceIoControl;
        use windows::core::HSTRING;

        #[repr(C)] #[derive(Default, Clone, Copy)]
        struct UsnJournalData { usn_journal_id: i64, first_usn: i64, next_usn: i64, lowest_valid_usn: i64, max_usn: i64, max_size: i64, allocation_delta: i64 }

        let volume_path = media::get_volume_path(root);
        let root_h = HSTRING::from(volume_path.as_str());
        let handle = match unsafe { CreateFileW(&root_h, GENERIC_READ.0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, None) } {
            Ok(h) => h,
            Err(e) => { warn!("CreateFileW failed for {}: {:?}", volume_path, e); return Ok(None); }
        };
        if handle == INVALID_HANDLE_VALUE { warn!("INVALID_HANDLE_VALUE for {}", volume_path); return Ok(None); }

        let mut journal_data = UsnJournalData::default();
        let mut bytes_returned = 0u32;
        let result = unsafe { DeviceIoControl(handle, FSCTL_QUERY_USN_JOURNAL, None, 0, Some(&mut journal_data as *mut _ as *mut _), std::mem::size_of::<UsnJournalData>() as u32, Some(&mut bytes_returned), None) };
        let _ = unsafe { CloseHandle(handle) };

        match result {
            Ok(_) => { info!("USN journal queried for {}, next_usn={}", volume_path, journal_data.next_usn); Ok(Some(journal_data.next_usn as u64)) }
            Err(e) => { warn!("DeviceIoControl failed for {}: {:?}", volume_path, e); Ok(None) }
        }
    }

    #[cfg(not(windows))]
    pub fn query_usn_journal(_root: &str) -> Result<Option<u64>> { Ok(None) }

    fn try_scan_mft(root: &str, opts: &ScanOpts) -> Result<Vec<FileEntry>> {
        use mft::MftParser;
        let volume_path = media::get_volume_path(root);
        let mut parser = MftParser::from_path(&volume_path)?;
        let entry_count = parser.get_entry_count();
        info!("MFT entries: {}", entry_count);
        let mut entries = Vec::new();

        for i in 0..entry_count {
            let entry = match parser.get_entry(i) { Ok(e) => e, Err(_) => continue };
            if !entry.is_allocated() || entry.is_dir() { continue; }
            let filename_attr = match entry.find_best_name_attribute() { Some(attr) => attr, None => continue };
            let filename = filename_attr.name.clone();
            let parent_frn = filename_attr.parent.entry;
            let size = filename_attr.logical_size as i64;
            let attributes = filename_attr.flags.bits() as i64;

            if opts.skip_system && (attributes as u32 & 0x04) != 0 { continue; }
            if opts.skip_hidden && (attributes as u32 & 0x02) != 0 { continue; }

            if !opts.includes.is_empty() {
                if let Some(ref e) = Path::new(&filename).extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()) {
                    if !opts.includes.iter().any(|p| glob::Pattern::new(p).map(|pat| pat.matches(e)).unwrap_or(false)) { continue; }
                }
            }
            if !opts.excludes.is_empty() {
                if let Some(ref e) = Path::new(&filename).extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()) {
                    if opts.excludes.iter().any(|p| glob::Pattern::new(p).map(|pat| pat.matches(e)).unwrap_or(false)) { continue; }
                }
            }

            let ext = Path::new(&filename).extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase());
            entries.push(FileEntry {
                filename,
                path_rel: String::new(),
                extension: ext,
                size,
                created: Some(filename_attr.created.timestamp()),
                modified: Some(filename_attr.modified.timestamp()),
                accessed: Some(filename_attr.accessed.timestamp()),
                attributes: Some(attributes),
                fs_specific: Some(FsSpecificData::Ntfs { frn: i, parent_frn }),
            });
        }
        info!("NTFS MFT scan complete: {} files", entries.len());
        Ok(entries)
    }
}

impl FileSystem for NtfsFileSystem {
    fn fs_type(&self) -> FsType { FsType::Ntfs }
    fn supports_incremental(&self) -> bool { true }

    fn scan(&self, root: &str, opts: &ScanOpts) -> Result<Vec<FileEntry>> {
        let media_type = media::detect_media_type(root);
        info!("detected media type for {}: {:?}", root, media_type);

        if media::is_subst_drive(root) {
            info!("subst drive detected — using walkdir (MFT unavailable for subst aliases)");
            return GenericFileSystem::new().scan(root, opts);
        }

        match media_type {
            MediaType::Vhd => {
                info!("VHD detected — using walkdir (MFT unavailable on virtual volumes)");
                GenericFileSystem::new().scan(root, opts)
            }
            MediaType::Hdd | MediaType::Ssd | MediaType::Usb | MediaType::Unknown => {
                match Self::try_scan_mft(root, opts) {
                    Ok(entries) => { info!("MFT scan succeeded for {}", root); Ok(entries) }
                    Err(e) => {
                        warn!("MFT scan failed for {}: {:?}. Falling back to walkdir.", root, e);
                        GenericFileSystem::new().scan(root, opts)
                    }
                }
            }
        }
    }

    fn scan_incremental(&self, root: &str, since: Option<u64>, opts: &ScanOpts) -> Result<(Vec<FileEntry>, Option<u64>)> {
        let current = Self::query_usn_journal(root)?;
        match (since, current) {
            (Some(_), Some(cur)) => { info!("incremental scan from USN {:?} to {}", since, cur); Ok((Vec::new(), current)) }
            _ => { warn!("USN journal not available, falling back to full scan"); let entries = self.scan(root, opts)?; Ok((entries, current)) }
        }
    }

    fn current_position(&self, root: &str) -> Result<Option<u64>> { Self::query_usn_journal(root) }
}

