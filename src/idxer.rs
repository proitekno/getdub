use crate::config;
use crate::fdb::{Fdb, FileMeta, VolumeInfo};
use crate::fs::{self, ScanOpts};
use anyhow::Result;
use log::{info, warn};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub struct Indexer {
    skip_hidden: bool, skip_system: bool, includes: Vec<String>, excludes: Vec<String>, compute_hashes: bool,
}

impl Indexer {
    pub fn new(include_str: Option<&str>, exclude_str: Option<&str>, skip_hidden: bool, skip_system: bool, compute_hashes: bool) -> Result<Self> {
        let parse = |s: Option<&str>| -> Vec<String> {
            s.map(|s| s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()).unwrap_or_default()
        };
        Ok(Indexer { skip_hidden, skip_system, includes: parse(include_str), excludes: parse(exclude_str), compute_hashes })
    }

    pub fn index_drive(&self, fdb: &mut Fdb, drive: &str) -> Result<()> {
        let root = normalize_drive_root(drive);
        if !Path::new(&root).exists() { anyhow::bail!("drive {} does not exist", root); }
        let fs = fs::detect_fs(&root);
        info!("detected filesystem: {:?}", fs.fs_type());
        let vol_type = match fs.fs_type() {
            fs::FsType::Ntfs => config::volume_type::NTFS,
            fs::FsType::Fat32 => config::volume_type::FAT32,
            fs::FsType::ExFat => config::volume_type::EXFAT,
            _ => config::volume_type::UNKNOWN,
        };
        let vol_info = VolumeInfo { path: root.clone(), volume_type: vol_type, label: None };
        let volume_id = fdb.ensure_volume(&vol_info)?;
        let scan_id = fdb.begin_scan(volume_id)?;
        let opts = ScanOpts { skip_hidden: self.skip_hidden, skip_system: self.skip_system, includes: self.includes.clone(), excludes: self.excludes.clone() };
        let entries = fs.scan(&root, &opts)?;
        let mut batch = fdb.begin_batch()?;
        let mut scanned = 0i64; let mut added = 0i64;
        for entry in entries {
            let meta = FileMeta { filename: entry.filename, path_rel: entry.path_rel, extension: entry.extension, size: entry.size, created: entry.created, modified: entry.modified, accessed: entry.accessed, attributes: entry.attributes, usn: None };
            if let Err(e) = batch.upsert_file(volume_id, &meta) { warn!("upsert failed: {:?}", e); continue; }
            scanned += 1;
            if batch.inserted + batch.updated >= config::BATCH_SIZE as i64 {
                added += batch.inserted; batch.commit()?; batch = fdb.begin_batch()?;
            }
        }
        added += batch.inserted; batch.commit()?;
        if let Some(usn) = fs.current_position(&root)? {
            let _ = fdb.set_last_usn(volume_id, usn as i64);
            info!("stored last_usn = {} for volume {}", usn, root);
        }
        fdb.update_scan_checkpoint(scan_id, scanned, added, 0, 0, None)?;
        fdb.finish_scan_ok(scan_id)?;
        info!("indexing {} done: {} files", root, scanned);
        Ok(())
    }

    pub fn index_drive_incremental(&self, fdb: &mut Fdb, drive: &str) -> Result<()> {
        let root = normalize_drive_root(drive);
        let fs = fs::detect_fs(&root);
        if !fs.supports_incremental() {
            info!("filesystem does not support incremental scan, running full scan");
            return self.index_drive(fdb, drive);
        }
        let vol_info = VolumeInfo { path: root.clone(), volume_type: config::volume_type::NTFS, label: None };
        let volume_id = fdb.ensure_volume(&vol_info)?;
        let last_usn = fdb.get_last_usn(volume_id)?;
        let opts = ScanOpts { skip_hidden: self.skip_hidden, skip_system: self.skip_system, includes: self.includes.clone(), excludes: self.excludes.clone() };
        let (entries, new_usn) = fs.scan_incremental(&root, last_usn.map(|u| u as u64), &opts)?;
        let mut batch = fdb.begin_batch()?;
        let mut scanned = 0i64;
        for entry in entries {
            let meta = FileMeta { filename: entry.filename, path_rel: entry.path_rel, extension: entry.extension, size: entry.size, created: entry.created, modified: entry.modified, accessed: entry.accessed, attributes: entry.attributes, usn: None };
            if let Err(e) = batch.upsert_file(volume_id, &meta) { warn!("upsert failed: {:?}", e); continue; }
            scanned += 1;
        }
        batch.commit()?;
        if let Some(usn) = new_usn {
            let _ = fdb.set_last_usn(volume_id, usn as i64);
            info!("updated last_usn to {}", usn);
        }
        info!("incremental scan complete: {} changes", scanned);
        Ok(())
    }

    pub fn compute_hashes_for_volume(&self, fdb: &Fdb, volume_id: i64) -> Result<()> {
        use rayon::prelude::*;
        let files = fdb.files_without_hashes(volume_id, config::HASH_QUERY_LIMIT)?;
        if files.is_empty() { info!("no files to hash"); return Ok(()); }
        info!("computing hashes for {} files", files.len());
        let results: Vec<_> = files.par_iter().filter_map(|(id, path, size)| {
            let p = Path::new(path);
            let partial = Self::compute_partial_hash(p).ok().flatten();
            let full = if *size > config::PARTIAL_HASH_SIZE as i64 { Self::compute_full_hash(p).ok().flatten() } else { partial.clone() };
            if partial.is_none() && full.is_none() { return None; }
            Some((*id, partial, full))
        }).collect();
        let mut updated = 0usize;
        for (id, partial, full) in &results {
            let status = match (partial, full) {
                (Some(_), Some(_)) => config::hash_status::FULL,
                (Some(_), None) => config::hash_status::PARTIAL,
                _ => config::hash_status::NONE,
            };
            if let Err(e) = fdb.update_file_hashes(*id, partial.as_deref(), full.as_deref(), status) {
                warn!("update hashes failed for id={}: {:?}", id, e); continue;
            }
            updated += 1;
        }
        info!("hashes computed for {} files", updated);
        Ok(())
    }

    fn compute_partial_hash(path: &Path) -> Result<Option<Vec<u8>>> {
        let mut file = File::open(path)?;
        let mut buffer = vec![0u8; config::PARTIAL_HASH_SIZE];
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 { return Ok(None); }
        let hash = xxhash_rust::xxh3::xxh3_64(&buffer[..bytes_read]);
        Ok(Some(hash.to_le_bytes().to_vec()))
    }

    fn compute_full_hash(path: &Path) -> Result<Option<Vec<u8>>> {
        let file = File::open(path)?;
        let meta = file.metadata()?;
        if meta.len() == 0 { return Ok(None); }
        if meta.len() < config::MMAP_SIZE_THRESHOLD {
            let mmap = unsafe { memmap2::Mmap::map(&file)? };
            let hash = blake3::hash(&mmap);
            Ok(Some(hash.as_bytes().to_vec()))
        } else {
            let mut hasher = blake3::Hasher::new();
            let mut reader = std::io::BufReader::with_capacity(config::STREAMING_BUF_SIZE, file);
            std::io::copy(&mut reader, &mut hasher)?;
            let hash = hasher.finalize();
            Ok(Some(hash.as_bytes().to_vec()))
        }
    }
}

pub fn normalize_drive_root(s: &str) -> String {
    let s = s.trim();
    if s.len() == 1 && s.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false) { format!("{}:\\", s.to_ascii_uppercase()) }
    else if s.len() == 2 && s.ends_with(':') { format!("{}\\", s.to_ascii_uppercase()) }
    else { s.to_string() }
}

