#[cfg(windows)]
pub fn is_running_as_admin() -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );

        let _ = windows::Win32::Foundation::CloseHandle(token);
        ok.is_ok() && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_running_as_admin() -> bool {
    unsafe { libc::geteuid() == 0 }
}

pub fn require_admin_or_exit() {
    if !is_running_as_admin() {
        eprintln!("[getdub] ERROR: administrator privileges required.");
        eprintln!();
        #[cfg(windows)]
        {
            eprintln!("Run from elevated PowerShell:");
            eprintln!("  Start-Process -Verb RunAs getdub.exe -- <args>");
        }
        #[cfg(not(windows))]
        eprintln!("Run with: sudo getdub <args>");
        std::process::exit(1);
    }
}

