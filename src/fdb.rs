use crate::config::{self, hash_status, hash_type, scan_status};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::path::{Path, PathBuf};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS volumes (id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, volume_type INTEGER NOT NULL, label TEXT, total_files INTEGER NOT NULL DEFAULT 0, indexed_at INTEGER NOT NULL, last_scan INTEGER, last_usn INTEGER, config_json TEXT);
CREATE TABLE IF NOT EXISTS files (id INTEGER PRIMARY KEY, volume_id INTEGER NOT NULL REFERENCES volumes(id), parent_id INTEGER, filename TEXT NOT NULL, path_rel TEXT NOT NULL, extension TEXT, size INTEGER NOT NULL, created INTEGER, modified INTEGER, accessed INTEGER, attributes INTEGER, partial_hash BLOB, full_hash BLOB, hash_status INTEGER NOT NULL DEFAULT 0, indexed_at INTEGER NOT NULL, last_verified INTEGER, usn INTEGER, is_deleted INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
CREATE TABLE IF NOT EXISTS file_tags (file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE, tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE, PRIMARY KEY (file_id, tag_id));
CREATE TABLE IF NOT EXISTS duplicate_groups (id INTEGER PRIMARY KEY, hash_type INTEGER NOT NULL, hash_value BLOB NOT NULL, file_count INTEGER NOT NULL, total_size INTEGER NOT NULL, created_at INTEGER NOT NULL, verified_at INTEGER);
CREATE TABLE IF NOT EXISTS duplicate_members (group_id INTEGER NOT NULL REFERENCES duplicate_groups(id) ON DELETE CASCADE, file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE, PRIMARY KEY (group_id, file_id));
CREATE TABLE IF NOT EXISTS ignored (id INTEGER PRIMARY KEY, volume_id INTEGER NOT NULL REFERENCES volumes(id), path TEXT NOT NULL, reason TEXT NOT NULL, indexed_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS scan_log (id INTEGER PRIMARY KEY, volume_id INTEGER NOT NULL REFERENCES volumes(id), started_at INTEGER NOT NULL, finished_at INTEGER, files_scanned INTEGER NOT NULL DEFAULT 0, files_added INTEGER NOT NULL DEFAULT 0, files_updated INTEGER NOT NULL DEFAULT 0, files_deleted INTEGER NOT NULL DEFAULT 0, last_checkpoint_usn INTEGER, status TEXT NOT NULL DEFAULT 'running', error_message TEXT);
CREATE INDEX IF NOT EXISTS idx_files_volume ON files(volume_id);
CREATE INDEX IF NOT EXISTS idx_files_parent ON files(parent_id);
CREATE INDEX IF NOT EXISTS idx_files_size ON files(size);
CREATE INDEX IF NOT EXISTS idx_files_ext ON files(extension);
CREATE INDEX IF NOT EXISTS idx_files_path ON files(volume_id, path_rel);
CREATE INDEX IF NOT EXISTS idx_files_partial ON files(partial_hash) WHERE partial_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_files_full ON files(full_hash) WHERE full_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_files_alive ON files(volume_id) WHERE is_deleted = 0;
CREATE INDEX IF NOT EXISTS idx_dup_groups_hash ON duplicate_groups(hash_value, hash_type);
CREATE INDEX IF NOT EXISTS idx_ignored_volume ON ignored(volume_id);
CREATE INDEX IF NOT EXISTS idx_scan_log_volume ON scan_log(volume_id);
"#;

pub struct Fdb { conn: Connection, path: PathBuf }

#[derive(Debug, Clone)]
pub struct FileMeta {
    pub filename: String, pub path_rel: String, pub extension: Option<String>,
    pub size: i64, pub created: Option<i64>, pub modified: Option<i64>,
    pub accessed: Option<i64>, pub attributes: Option<i64>, pub usn: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub enum IgnoreReason { Permission, IoError, SymlinkBroken, System, Hidden }

impl std::fmt::Display for IgnoreReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            IgnoreReason::Permission => "permission", IgnoreReason::IoError => "ioerror",
            IgnoreReason::SymlinkBroken => "symlink_broken", IgnoreReason::System => "system",
            IgnoreReason::Hidden => "hidden",
        })
    }
}

#[derive(Debug, Clone)]
pub struct VolumeInfo { pub path: String, pub volume_type: i32, pub label: Option<String> }

#[derive(Debug, serde::Serialize)]
pub struct FileRow {
    pub id: i64, pub volume: String, pub path: String, pub filename: String,
    pub extension: Option<String>, pub size: i64, pub modified: Option<i64>,
    pub partial_hash: Option<String>, pub full_hash: Option<String>,
    pub hash_status: i32, pub is_deleted: i32,
}

#[derive(Debug)]
pub struct DupGroup {
    pub hash_type: i32, pub hash_hex: String, pub file_count: i32,
    pub total_size: i64, pub members: Vec<(i64, String)>,
}

impl Fdb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?; }
        let conn = Connection::open(path).with_context(|| format!("open sqlite {}", path.display()))?;
        conn.execute_batch(config::sqlite_pragma_init())?;
        let fdb = Fdb { conn, path: path.to_path_buf() };
        fdb.conn.execute_batch(SCHEMA)?;
        Ok(fdb)
    }
    pub fn path(&self) -> &Path { &self.path }
    
    pub fn ensure_volume(&self, info: &VolumeInfo) -> Result<i64> {
        let now = Utc::now().timestamp();
        self.conn.execute("INSERT INTO volumes (path, volume_type, label, indexed_at, last_scan) VALUES (?1, ?2, ?3, ?4, ?4) ON CONFLICT(path) DO UPDATE SET last_scan = ?4, label = COALESCE(excluded.label, volumes.label)", params![info.path, info.volume_type, info.label, now])?;
        Ok(self.conn.query_row("SELECT id FROM volumes WHERE path = ?1", params![info.path], |r| r.get(0))?)
    }
    
    pub fn get_volume_id(&self, path: &str) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT id FROM volumes WHERE path = ?1", params![path], |r| r.get(0)).optional()?)
    }
    
    pub fn get_last_usn(&self, volume_id: i64) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT last_usn FROM volumes WHERE id = ?1", params![volume_id], |r| r.get(0)).optional()?)
    }
    
    pub fn set_last_usn(&self, volume_id: i64, usn: i64) -> Result<()> {
        self.conn.execute("UPDATE volumes SET last_usn = ?2 WHERE id = ?1", params![volume_id, usn])?; Ok(())
    }
    
    pub fn begin_scan(&self, volume_id: i64) -> Result<i64> {
        let now = Utc::now().timestamp();
        self.conn.execute("INSERT INTO scan_log (volume_id, started_at, status) VALUES (?1, ?2, ?3)", params![volume_id, now, scan_status::RUNNING])?;
        Ok(self.conn.last_insert_rowid())
    }
    
    pub fn update_scan_checkpoint(&self, scan_id: i64, scanned: i64, added: i64, updated: i64, deleted: i64, last_usn: Option<i64>) -> Result<()> {
        self.conn.execute("UPDATE scan_log SET files_scanned = ?2, files_added = ?3, files_updated = ?4, files_deleted = ?5, last_checkpoint_usn = ?6 WHERE id = ?1", params![scan_id, scanned, added, updated, deleted, last_usn])?; Ok(())
    }
    
    pub fn finish_scan_ok(&self, scan_id: i64) -> Result<()> {
        let now = Utc::now().timestamp();
        self.conn.execute("UPDATE scan_log SET finished_at = ?2, status = ?3 WHERE id = ?1", params![scan_id, now, scan_status::DONE])?; Ok(())
    }
    
    pub fn finish_scan_err(&self, scan_id: i64, err: &str) -> Result<()> {
        let now = Utc::now().timestamp();
        self.conn.execute("UPDATE scan_log SET finished_at = ?2, status = ?3, error_message = ?4 WHERE id = ?1", params![scan_id, now, scan_status::FAILED, err])?; Ok(())
    }
    
    pub fn begin_batch(&mut self) -> Result<Batch<'_>> {
        let tx = self.conn.transaction()?;
        Ok(Batch { tx, inserted: 0, updated: 0 })
    }
    
    pub fn insert_ignored(&self, volume_id: i64, path: &str, reason: IgnoreReason) -> Result<()> {
        let now = Utc::now().timestamp();
        self.conn.execute("INSERT INTO ignored (volume_id, path, reason, indexed_at) VALUES (?1, ?2, ?3, ?4)", params![volume_id, path, reason.to_string(), now])?; Ok(())
    }
    
    pub fn update_file_hashes(&self, file_id: i64, partial_hash: Option<&[u8]>, full_hash: Option<&[u8]>, hash_status: i32) -> Result<()> {
        self.conn.execute("UPDATE files SET partial_hash = ?2, full_hash = ?3, hash_status = ?4 WHERE id = ?1", params![file_id, partial_hash, full_hash, hash_status])?; Ok(())
    }
    
    pub fn files_without_hashes(&self, volume_id: i64, limit: usize) -> Result<Vec<(i64, String, i64)>> {
        let mut stmt = self.conn.prepare("SELECT f.id, v.path || f.path_rel, f.size FROM files f JOIN volumes v ON v.id = f.volume_id WHERE f.volume_id = ?1 AND f.hash_status < ?2 AND f.is_deleted = 0 ORDER BY f.size DESC LIMIT ?3")?;
        let rows: Vec<(i64, String, i64)> = stmt.query_map(params![volume_id, hash_status::FULL, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }
    
    pub fn ensure_tag(&self, name: &str) -> Result<i64> {
        self.conn.execute("INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING", params![name])?;
        Ok(self.conn.query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| r.get(0))?)
    }
    
    pub fn tag_file(&self, file_id: i64, tag_id: i64) -> Result<()> {
        self.conn.execute("INSERT OR IGNORE INTO file_tags (file_id, tag_id) VALUES (?1, ?2)", params![file_id, tag_id])?; Ok(())
    }
    
    pub fn soft_delete_volume(&self, volume_id: i64) -> Result<()> {
        self.conn.execute("UPDATE files SET is_deleted = 1 WHERE volume_id = ?1", params![volume_id])?; Ok(())
    }
    
    pub fn export_csv(&self, out: &Path) -> Result<u64> {
        let mut wtr = csv::Writer::from_path(out)?;
        let mut stmt = self.conn.prepare("SELECT f.id, v.path, f.path_rel, f.filename, f.extension, f.size, f.modified, f.partial_hash, f.full_hash, f.hash_status, f.is_deleted FROM files f JOIN volumes v ON v.id = f.volume_id ORDER BY v.path, f.path_rel")?;
        let rows = stmt.query_map([], |r| {
            let partial: Option<Vec<u8>> = r.get(7)?; let full: Option<Vec<u8>> = r.get(8)?;
            Ok(FileRow { id: r.get(0)?, volume: r.get(1)?, path: r.get(2)?, filename: r.get(3)?, extension: r.get(4)?, size: r.get(5)?, modified: r.get(6)?, partial_hash: partial.as_deref().map(hex), full_hash: full.as_deref().map(hex), hash_status: r.get(9)?, is_deleted: r.get(10)? })
        })?;
        wtr.write_record(["id", "volume", "path", "filename", "extension", "size", "modified", "partial_hash", "full_hash", "hash_status", "is_deleted"])?;
        let mut count = 0u64;
        for row in rows { wtr.serialize(&row?)?; count += 1; }
        wtr.flush()?; Ok(count)
    }
    
    pub fn find_duplicates_full(&self, limit: usize) -> Result<Vec<DupGroup>> {
        let mut stmt = self.conn.prepare("SELECT full_hash, COUNT(*) AS cnt, SUM(size) AS total FROM files WHERE full_hash IS NOT NULL AND is_deleted = 0 GROUP BY full_hash HAVING cnt > 1 ORDER BY total DESC LIMIT ?1")?;
        let groups: Vec<(Vec<u8>, i32, i64)> = stmt.query_map(params![limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        let mut out = Vec::with_capacity(groups.len());
        for (hash, cnt, total) in groups {
            let members = self.members_by_hash(hash_type::FULL, &hash)?;
            out.push(DupGroup { hash_type: hash_type::FULL, hash_hex: hex(&hash), file_count: cnt, total_size: total, members });
        }
        Ok(out)
    }
    
    pub fn find_duplicates_partial(&self, limit: usize) -> Result<Vec<DupGroup>> {
        let mut stmt = self.conn.prepare("SELECT size, partial_hash, COUNT(*) AS cnt FROM files WHERE partial_hash IS NOT NULL AND is_deleted = 0 GROUP BY size, partial_hash HAVING cnt > 1 ORDER BY cnt DESC LIMIT ?1")?;
        let groups: Vec<(i64, Vec<u8>, i32)> = stmt.query_map(params![limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        let mut out = Vec::with_capacity(groups.len());
        for (size, hash, cnt) in groups {
            let members = self.members_by_hash(hash_type::PARTIAL, &hash)?;
            out.push(DupGroup { hash_type: hash_type::PARTIAL, hash_hex: hex(&hash), file_count: cnt, total_size: size * cnt as i64, members });
        }
        Ok(out)
    }
    
    fn members_by_hash(&self, h_type: i32, hash: &[u8]) -> Result<Vec<(i64, String)>> {
        let col = if h_type == hash_type::FULL { "full_hash" } else { "partial_hash" };
        let sql = format!("SELECT f.id, v.path || f.path_rel FROM files f JOIN volumes v ON v.id = f.volume_id WHERE f.{col} = ?1 AND f.is_deleted = 0 ORDER BY f.path_rel");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<(i64, String)> = stmt.query_map(params![hash], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }
    
    pub fn stats(&self) -> Result<()> {
        let volumes: i64 = self.conn.query_row("SELECT COUNT(*) FROM volumes", [], |r| r.get(0))?;
        let files: i64 = self.conn.query_row("SELECT COUNT(*) FROM files WHERE is_deleted = 0", [], |r| r.get(0))?;
        let with_partial: i64 = self.conn.query_row("SELECT COUNT(*) FROM files WHERE partial_hash IS NOT NULL AND is_deleted = 0", [], |r| r.get(0))?;
        let with_full: i64 = self.conn.query_row("SELECT COUNT(*) FROM files WHERE full_hash IS NOT NULL AND is_deleted = 0", [], |r| r.get(0))?;
        let ignored: i64 = self.conn.query_row("SELECT COUNT(*) FROM ignored", [], |r| r.get(0))?;
        let db_size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        println!("[fdb] path         : {}", self.path.display());
        println!("[fdb] db size      : {} MB", db_size / 1024 / 1024);
        println!("[fdb] volumes      : {}", volumes);
        println!("[fdb] files        : {}", files);
        println!("[fdb] hashed part. : {}", with_partial);
        println!("[fdb] hashed full  : {}", with_full);
        println!("[fdb] ignored      : {}", ignored);
        Ok(())
    }
    
    pub fn integrity_check(&self) -> Result<bool> {
        let result: String = self.conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        Ok(result == "ok")
    }
    
    pub fn count_alive_files(&self, volume_id: Option<i64>) -> Result<i64> {
        Ok(match volume_id {
            Some(vid) => self.conn.query_row("SELECT COUNT(*) FROM files WHERE volume_id = ?1 AND is_deleted = 0", params![vid], |r| r.get(0))?,
            None => self.conn.query_row("SELECT COUNT(*) FROM files WHERE is_deleted = 0", [], |r| r.get(0))?,
        })
    }
    
    pub fn count_hashed(&self, volume_id: Option<i64>, min_status: i32) -> Result<i64> {
        Ok(match volume_id {
            Some(vid) => self.conn.query_row("SELECT COUNT(*) FROM files WHERE volume_id = ?1 AND is_deleted = 0 AND hash_status >= ?2", params![vid, min_status], |r| r.get(0))?,
            None => self.conn.query_row("SELECT COUNT(*) FROM files WHERE is_deleted = 0 AND hash_status >= ?1", params![min_status], |r| r.get(0))?,
        })
    }
    
    pub fn count_dup_groups_full(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM (SELECT full_hash FROM files WHERE full_hash IS NOT NULL AND is_deleted = 0 GROUP BY full_hash HAVING COUNT(*) > 1)", [], |r| r.get(0))?)
    }
    
    pub fn max_dup_group_members(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COALESCE(MAX(cnt), 0) FROM (SELECT COUNT(*) AS cnt FROM files WHERE full_hash IS NOT NULL AND is_deleted = 0 GROUP BY full_hash HAVING COUNT(*) > 1)", [], |r| r.get(0))?)
    }
    
    pub fn count_ignored(&self, volume_id: Option<i64>) -> Result<i64> {
        Ok(match volume_id {
            Some(vid) => self.conn.query_row("SELECT COUNT(*) FROM ignored WHERE volume_id = ?1", params![vid], |r| r.get(0))?,
            None => self.conn.query_row("SELECT COUNT(*) FROM ignored", [], |r| r.get(0))?,
        })
    }
}

pub struct Batch<'a> { tx: Transaction<'a>, pub inserted: i64, pub updated: i64 }

impl<'a> Batch<'a> {
    pub fn upsert_file(&mut self, volume_id: i64, m: &FileMeta) -> Result<()> {
        let now = Utc::now().timestamp();
        let existing: Option<(i64, Option<i64>)> = self.tx.query_row("SELECT id, modified FROM files WHERE volume_id = ?1 AND path_rel = ?2", params![volume_id, m.path_rel], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        match existing {
            Some((id, Some(old_mod))) if old_mod == m.modified.unwrap_or(0) => {
                self.tx.execute("UPDATE files SET last_verified = ?2, is_deleted = 0, size = ?3, attributes = ?4 WHERE id = ?1", params![id, now, m.size, m.attributes])?;
                self.updated += 1;
            }
            Some((id, _)) => {
                self.tx.execute("UPDATE files SET size = ?2, modified = ?3, created = ?4, accessed = ?5, attributes = ?6, usn = ?7, partial_hash = NULL, full_hash = NULL, hash_status = ?8, last_verified = ?9, is_deleted = 0 WHERE id = ?1", params![id, m.size, m.modified, m.created, m.accessed, m.attributes, m.usn, hash_status::NONE, now])?;
                self.updated += 1;
            }
            None => {
                self.tx.execute("INSERT INTO files (volume_id, filename, path_rel, extension, size, created, modified, accessed, attributes, usn, indexed_at, last_verified, hash_status, is_deleted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12, 0)", params![volume_id, m.filename, m.path_rel, m.extension, m.size, m.created, m.modified, m.accessed, m.attributes, m.usn, now, hash_status::NONE])?;
                self.inserted += 1;
            }
        }
        Ok(())
    }
    pub fn insert_ignored(&mut self, volume_id: i64, path: &str, reason: IgnoreReason) -> Result<()> {
        let now = Utc::now().timestamp();
        self.tx.execute("INSERT INTO ignored (volume_id, path, reason, indexed_at) VALUES (?1, ?2, ?3, ?4)", params![volume_id, path, reason.to_string(), now])?; Ok(())
    }
    pub fn commit(self) -> Result<()> { self.tx.commit()?; Ok(()) }
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b { s.push_str(&format!("{:02x}", byte)); }
    s
}