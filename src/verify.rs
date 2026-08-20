use crate::config::{exit_code, hash_status};
use crate::fdb::Fdb;

pub struct VerifyOpts {
    pub volume: Option<String>,
    pub expect_files_min: Option<i64>,
    pub expect_files_max: Option<i64>,
    pub expect_hashed_pct: Option<i64>,
    pub expect_dup_groups: Option<i64>,
    pub expect_dup_members_min: Option<i64>,
    pub expect_usn: bool,
    pub integrity: bool,
}

struct Check {
    name: &'static str,
    code: i32,
    passed: bool,
    detail: String,
}

impl Check {
    fn pass(name: &'static str, code: i32, detail: String) -> Self {
        Check { name, code, passed: true, detail }
    }
    fn fail(name: &'static str, code: i32, detail: String) -> Self {
        Check { name, code, passed: false, detail }
    }
}

pub fn run(fdb: &Fdb, opts: &VerifyOpts) -> i32 {
    let mut checks: Vec<Check> = Vec::new();

    if opts.integrity {
        match fdb.integrity_check() {
            Ok(true) => checks.push(Check::pass("db_integrity", exit_code::VERIFY_DB_INTEGRITY, "PRAGMA integrity_check = ok".into())),
            Ok(false) => checks.push(Check::fail("db_integrity", exit_code::VERIFY_DB_INTEGRITY, "PRAGMA integrity_check вернул ошибку".into())),
            Err(e) => checks.push(Check::fail("db_integrity", exit_code::VERIFY_DB_INTEGRITY, format!("ошибка запроса: {:?}", e))),
        }
    }

    let volume_id: Option<i64> = if let Some(v) = &opts.volume {
        let normalized = crate::idxer::normalize_drive_root(v);
        match fdb.get_volume_id(&normalized) {
            Ok(Some(id)) => {
                checks.push(Check::pass("volume_exists", exit_code::VERIFY_VOLUME_MISSING, format!("том '{}' найден (id={})", normalized, id)));
                Some(id)
            }
            Ok(None) => {
                checks.push(Check::fail("volume_exists", exit_code::VERIFY_VOLUME_MISSING, format!("том '{}' не найден в БД", normalized)));
                None
            }
            Err(e) => {
                checks.push(Check::fail("volume_exists", exit_code::VERIFY_VOLUME_MISSING, format!("ошибка запроса тома: {:?}", e)));
                None
            }
        }
    } else { None };

    if opts.expect_files_min.is_some() || opts.expect_files_max.is_some() {
        match fdb.count_alive_files(volume_id) {
            Ok(n) => {
                let min_ok = opts.expect_files_min.map(|m| n >= m).unwrap_or(true);
                let max_ok = opts.expect_files_max.map(|m| n <= m).unwrap_or(true);
                let detail = format!("живых файлов: {} (ожидание: {:?}..{:?})", n, opts.expect_files_min, opts.expect_files_max);
                if min_ok && max_ok { checks.push(Check::pass("files_count", exit_code::VERIFY_FILES_COUNT, detail)); } 
                else { checks.push(Check::fail("files_count", exit_code::VERIFY_FILES_COUNT, detail)); }
            }
            Err(e) => checks.push(Check::fail("files_count", exit_code::VERIFY_FILES_COUNT, format!("ошибка подсчёта: {:?}", e))),
        }
    }

    if let Some(pct) = opts.expect_hashed_pct {
        let total = fdb.count_alive_files(volume_id).unwrap_or(0);
        let hashed = fdb.count_hashed(volume_id, hash_status::FULL).unwrap_or(0);
        let actual_pct = if total > 0 { hashed * 100 / total } else { 0 };
        let detail = format!("хешировано: {}/{} ({}%, ожидание >= {}%)", hashed, total, actual_pct, pct);
        if total == 0 && pct == 0 || actual_pct >= pct { checks.push(Check::pass("hashes", exit_code::VERIFY_HASHES, detail)); } 
        else { checks.push(Check::fail("hashes", exit_code::VERIFY_HASHES, detail)); }
    }

    if let Some(expected) = opts.expect_dup_groups {
        match fdb.count_dup_groups_full() {
            Ok(n) => {
                let detail = format!("групп дубликатов: {} (ожидание: {})", n, expected);
                if n == expected { checks.push(Check::pass("dup_groups", exit_code::VERIFY_DUP_GROUPS, detail)); } 
                else { checks.push(Check::fail("dup_groups", exit_code::VERIFY_DUP_GROUPS, detail)); }
            }
            Err(e) => checks.push(Check::fail("dup_groups", exit_code::VERIFY_DUP_GROUPS, format!("ошибка подсчёта: {:?}", e))),
        }
    }

    if let Some(min_members) = opts.expect_dup_members_min {
        match fdb.max_dup_group_members() {
            Ok(n) => {
                let detail = format!("файлов в крупнейшей группе: {} (ожидание >= {})", n, min_members);
                if n >= min_members { checks.push(Check::pass("dup_members", exit_code::VERIFY_DUP_MEMBERS, detail)); } 
                else { checks.push(Check::fail("dup_members", exit_code::VERIFY_DUP_MEMBERS, detail)); }
            }
            Err(e) => checks.push(Check::fail("dup_members", exit_code::VERIFY_DUP_MEMBERS, format!("ошибка подсчёта: {:?}", e))),
        }
    }

    if opts.expect_usn {
        match volume_id {
            Some(vid) => match fdb.get_last_usn(vid) {
                Ok(Some(usn)) if usn > 0 => checks.push(Check::pass("usn_stored", exit_code::VERIFY_USN, format!("last_usn = {}", usn))),
                Ok(_) => checks.push(Check::fail("usn_stored", exit_code::VERIFY_USN, "last_usn не сохранён (инкрементальный скан не сработает)".into())),
                Err(e) => checks.push(Check::fail("usn_stored", exit_code::VERIFY_USN, format!("ошибка запроса: {:?}", e))),
            },
            None => checks.push(Check::fail("usn_stored", exit_code::VERIFY_USN, "не задан --volume, нечего проверять".into())),
        }
    }

    println!("\n=== VERIFY RESULTS ===");
    let mut first_fail_code: Option<i32> = None;
    for c in &checks {
        let mark = if c.passed { "[PASS]" } else { "[FAIL]" };
        println!("{:<6} {:<14} code={:<3} {}", mark, c.name, c.code, c.detail);
        if !c.passed && first_fail_code.is_none() { first_fail_code = Some(c.code); }
    }

    let passed = checks.iter().filter(|c| c.passed).count();
    let total = checks.len();
    println!("---\nпройдено: {}/{}", passed, total);

    match first_fail_code {
        Some(code) => { println!("VERIFY: FAIL (exit code {})", code); code }
        None => { println!("VERIFY: OK (exit code 0)"); exit_code::OK }
    }
}

