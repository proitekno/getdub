mod admin; mod config; mod fdb; mod fs; mod idxer; mod logger; mod testing; mod verify;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fdb::Fdb;
use idxer::Indexer;
use log::info;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "getdub", version, about = "Fast duplicate file finder with persistent index")]
struct Cli {
    #[arg(long, global = true)] db: Option<PathBuf>,
    #[arg(long, global = true, default_value = "info")] log_level: String,
    #[arg(short = 'v', long, global = true)] verbose: bool,
    #[arg(long, global = true, default_value_t = true)] skip_hidden: bool,
    #[arg(long, global = true, default_value_t = true)] skip_system: bool,
    #[arg(long, global = true)] hash: bool,
    #[command(subcommand)] command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Idx { #[command(subcommand)] cmd: IdxCmd },
    Fdb { #[command(subcommand)] cmd: FdbCmd },
    Verify {
        #[arg(long)] volume: Option<String>, #[arg(long)] expect_files_min: Option<i64>, #[arg(long)] expect_files_max: Option<i64>,
        #[arg(long)] expect_hashed_pct: Option<i64>, #[arg(long)] expect_dup_groups: Option<i64>,
        #[arg(long)] expect_dup_members_min: Option<i64>, #[arg(long)] expect_usn: bool, #[arg(long)] integrity: bool,
    },
    Usn { #[command(subcommand)] cmd: UsnCmd },
}

#[derive(Subcommand)]
enum IdxCmd {
    Drive { target: Option<String>, #[arg(long)] all: bool, #[arg(long)] incremental: bool, #[arg(long)] include: Option<String>, #[arg(long)] exclude: Option<String> },
}

#[derive(Subcommand)]
enum FdbCmd {
    Stats, Export { #[arg(long)] out: PathBuf },
    Query { #[arg(long)] duplicates: bool, #[arg(long)] candidates: bool, #[arg(long, default_value_t = 50)] limit: usize },
    Erase { #[arg(long)] volume: String },
}

#[derive(Subcommand)]
enum UsnCmd { Status { volume: String } }

fn init_logger(level: &str, verbose: bool) { logger::init_logger(level, verbose); }

#[cfg(windows)]
fn list_local_drives() -> Vec<String> {
    use crate::config::win::DRIVE_FIXED;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    use windows::core::HSTRING;
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if (mask >> i) & 1 == 0 { continue; }
        let letter = (b'A' as u32 + i) as u8;
        let root = format!("{}:\\", letter as char);
        let root_h = HSTRING::from(root.as_str());
        let dt = unsafe { GetDriveTypeW(&root_h) };
        if dt == DRIVE_FIXED { out.push(root); }
    }
    out
}

#[cfg(not(windows))]
fn list_local_drives() -> Vec<String> { vec!["/".to_string()] }

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logger(&cli.log_level, cli.verbose);
    let db_path = cli.db.clone().unwrap_or_else(config::default_db_path);
    info!("using database: {}", db_path.display());

    let needs_admin = matches!(&cli.command, Commands::Idx { .. } | Commands::Usn { .. });
    if needs_admin { admin::require_admin_or_exit(); }

    let mut fdb = Fdb::open(&db_path).with_context(|| format!("open fdb at {}", db_path.display()))?;

    match cli.command {
        Commands::Idx { cmd } => match cmd {
            IdxCmd::Drive { target, all, incremental, include, exclude } => {
                let indexer = Indexer::new(include.as_deref(), exclude.as_deref(), cli.skip_hidden, cli.skip_system, cli.hash)?;
                let drives: Vec<String> = if all { list_local_drives() } else if let Some(t) = target { vec![t] } else { anyhow::bail!("specify drive (e.g. 'C') or --all"); };
                for d in drives {
                    let normalized = idxer::normalize_drive_root(&d);
                    let result = if incremental { indexer.index_drive_incremental(&mut fdb, &d) } else { indexer.index_drive(&mut fdb, &d) };
                    if let Err(e) = result { eprintln!("[getdub] failed to index {}: {:?}", d, e); continue; }
                    if cli.hash {
                        let vol_info = fdb::VolumeInfo { path: normalized.clone(), volume_type: config::volume_type::NTFS, label: None };
                        if let Ok(vid) = fdb.ensure_volume(&vol_info) {
                            if let Err(e) = indexer.compute_hashes_for_volume(&fdb, vid) { eprintln!("[getdub] hashing failed for {}: {:?}", d, e); }
                        }
                    }
                }
            }
        },
        Commands::Fdb { cmd } => match cmd {
            FdbCmd::Stats => fdb.stats()?,
            FdbCmd::Export { out } => { let n = fdb.export_csv(&out)?; println!("[fdb] exported {} rows to {}", n, out.display()); }
            FdbCmd::Query { duplicates, candidates, limit } => {
                if duplicates {
                    let groups = fdb.find_duplicates_full(limit)?;
                    if groups.is_empty() { println!("no full-hash duplicates found"); }
                    for g in groups { println!("\n--- {} copies, {} bytes total, hash={} ---", g.file_count, g.total_size, g.hash_hex); for (id, path) in &g.members { println!("  [{}] {}", id, path); } }
                }
                if candidates {
                    let groups = fdb.find_duplicates_partial(limit)?;
                    if groups.is_empty() { println!("no partial-hash candidates found"); }
                    for g in groups { println!("\n--- {} candidates, hash={} ---", g.file_count, g.hash_hex); for (id, path) in &g.members { println!("  [{}] {}", id, path); } }
                }
                if !duplicates && !candidates { println!("use --duplicates or --candidates"); }
            }
            FdbCmd::Erase { volume } => {
                let normalized = idxer::normalize_drive_root(&volume);
                match fdb.get_volume_id(&normalized)? {
                    Some(vid) => { fdb.soft_delete_volume(vid)?; println!("[fdb] soft-deleted all files in volume {}", normalized); }
                    None => println!("[fdb] volume {} not found", normalized),
                }
            }
        },
        Commands::Verify { volume, expect_files_min, expect_files_max, expect_hashed_pct, expect_dup_groups, expect_dup_members_min, expect_usn, integrity } => {
            let opts = verify::VerifyOpts { volume, expect_files_min, expect_files_max, expect_hashed_pct, expect_dup_groups, expect_dup_members_min, expect_usn, integrity };
            let code = verify::run(&fdb, &opts);
            std::process::exit(code);
        }
        Commands::Usn { cmd } => match cmd {
            UsnCmd::Status { volume } => {
                let normalized = idxer::normalize_drive_root(&volume);
                match fs::ntfs::NtfsFileSystem::query_usn_journal(&normalized)? {
                    Some(usn) => { println!("[usn] USN Journal is ACTIVE on {}", normalized); println!("[usn] Current USN: {}", usn); }
                    None => { println!("[usn] USN Journal is NOT ACTIVE on {}", normalized); println!("[usn] Incremental scanning will fall back to full scan"); }
                }
            }
        }
    }
    Ok(())
}

