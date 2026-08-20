use super::{FileEntry, FsType, FileSystem, ScanOpts};
use anyhow::Result;
use log::{debug, info};
use std::path::Path;
use walkdir::WalkDir;

pub struct GenericFileSystem;
impl GenericFileSystem { pub fn new() -> Self { GenericFileSystem } }

impl FileSystem for GenericFileSystem {
    fn fs_type(&self) -> FsType { FsType::Unknown }
    fn supports_incremental(&self) -> bool { false }

    fn scan(&self, root: &str, opts: &ScanOpts) -> Result<Vec<FileEntry>> {
        let mut entries = Vec::new();
        let walker = WalkDir::new(root).follow_links(false).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !crate::config::is_skipped_dir(&name)
        });

        for entry in walker {
            let entry = match entry { Ok(e) => e, Err(e) => { debug!("walkdir error: {:?}", e); continue; } };
            if !entry.file_type().is_file() { continue; }

            let path = entry.path();
            let rel = match path.strip_prefix(root) { Ok(r) => r.to_string_lossy().replace('/', "\\"), Err(_) => continue };
            let rel = rel.trim_start_matches('\\').to_string();
            let filename = match entry.file_name().to_str() { Some(s) => s.to_string(), None => continue };

            if !opts.includes.is_empty() {
                if !opts.includes.iter().any(|p| glob::Pattern::new(p).map(|pat| pat.matches(&filename)).unwrap_or(false)) { continue; }
            }
            if !opts.excludes.is_empty() {
                if opts.excludes.iter().any(|p| glob::Pattern::new(p).map(|pat| pat.matches(&filename)).unwrap_or(false)) { continue; }
            }

            let meta = match std::fs::metadata(path) { Ok(m) => m, Err(_) => continue };
            let ext = Path::new(&filename).extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase());
            let created = meta.created().ok().map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0));
            let modified = meta.modified().ok().map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0));
            let accessed = meta.accessed().ok().map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0));

            entries.push(FileEntry { filename, path_rel: rel, extension: ext, size: meta.len() as i64, created, modified, accessed, attributes: None, fs_specific: None });
        }
        info!("generic scan complete: {} files", entries.len());
        Ok(entries)
    }

    fn scan_incremental(&self, root: &str, _since: Option<u64>, opts: &ScanOpts) -> Result<(Vec<FileEntry>, Option<u64>)> {
        let entries = self.scan(root, opts)?; Ok((entries, None))
    }
    fn current_position(&self, _root: &str) -> Result<Option<u64>> { Ok(None) }
}

