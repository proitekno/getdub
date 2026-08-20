pub const FDB_DIR_NAME: &str = "fdb";
pub const FDB_FILE_NAME: &str = "getdub.fdb";
pub const LOG_FILE_NAME: &str = "getdub.log";

pub const BATCH_SIZE: usize = 1000;
pub const CHECKPOINT_EVERY_N_BATCHES: usize = 10;
pub const HASH_QUERY_LIMIT: usize = 100_000;
pub const USN_BUFFER_SIZE: usize = 65_536;

pub const SKIP_DIRS: &[&str] = &[
    "$recycle.bin",
    "system volume information",
    "$windows.~bt",
    "$windows.~ws",
];

pub const PARTIAL_HASH_SIZE: usize = 4096;
pub const MMAP_SIZE_THRESHOLD: u64 = 64 * 1024 * 1024;
pub const STREAMING_BUF_SIZE: usize = 1024 * 1024;

pub const SQLITE_MMAP_SIZE: i64 = 256 * 1024 * 1024;
pub const SQLITE_CACHE_SIZE_KB: i64 = -2_000_000;
pub const SQLITE_BUSY_TIMEOUT_MS: i64 = 5000;

pub mod hash_status {
    pub const NONE: i32 = 0;
    pub const PARTIAL: i32 = 1;
    pub const FULL: i32 = 2;
}

pub mod volume_type {
    pub const UNKNOWN: i32 = 0;
    pub const NTFS: i32 = 1;
    pub const FAT32: i32 = 2;
    pub const EXFAT: i32 = 3;
    pub const ARCHIVE: i32 = 4;
}

pub mod hash_type {
    pub const PARTIAL: i32 = 1;
    pub const FULL: i32 = 2;
}

pub mod scan_status {
    pub const RUNNING: &str = "running";
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
}

pub mod exit_code {
    pub const OK: i32 = 0;
    pub const GENERAL: i32 = 1;
    pub const USAGE: i32 = 2;
    pub const VERIFY_DB_INTEGRITY: i32 = 10;
    pub const VERIFY_VOLUME_MISSING: i32 = 11;
    pub const VERIFY_FILES_COUNT: i32 = 12;
    pub const VERIFY_HASHES: i32 = 13;
    pub const VERIFY_DUP_GROUPS: i32 = 14;
    pub const VERIFY_DUP_MEMBERS: i32 = 15;
    pub const VERIFY_USN: i32 = 16;
    pub const VERIFY_IGNORED: i32 = 17;
}

#[cfg(windows)]
pub mod win {
    pub const FSCTL_QUERY_USN_JOURNAL: u32 = 0x000900F4;
    pub const FSCTL_READ_USN_JOURNAL: u32 = 0x000900BB;
    pub const DRIVE_FIXED: u32 = 3;
    pub const FILE_ATTRIBUTE_HIDDEN: u32 = 0x02;
    pub const FILE_ATTRIBUTE_SYSTEM: u32 = 0x04;
}

pub fn default_db_path() -> std::path::PathBuf {
    std::path::PathBuf::from(FDB_DIR_NAME).join(FDB_FILE_NAME)
}

pub fn sqlite_pragma_init() -> &'static str {
    concat!(
        "PRAGMA journal_mode       = WAL; ",
        "PRAGMA synchronous        = NORMAL; ",
        "PRAGMA temp_store         = MEMORY; ",
        "PRAGMA mmap_size          = 268435456; ",
        "PRAGMA cache_size         = -2000000; ",
        "PRAGMA busy_timeout       = 5000; ",
        "PRAGMA foreign_keys       = ON; ",
        "PRAGMA recursive_triggers = ON;"
    )
}

pub fn is_skipped_dir(name: &str) -> bool {
    let lower = name.to_lowercase();
    SKIP_DIRS.iter().any(|&d| d == lower)
}

