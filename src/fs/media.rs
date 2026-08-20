use log::{debug, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType { Hdd, Ssd, Vhd, Usb, Unknown }

const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D1400;
const STORAGE_ADAPTER_PROPERTY: u32 = 1;
const STORAGE_DEVICE_SEEK_PENALTY_PROPERTY: u32 = 7;
const PROPERTY_STANDARD_QUERY: u32 = 0;
const BUS_TYPE_USB: u32 = 7;
const BUS_TYPE_VIRTUAL: u32 = 14;
const BUS_TYPE_FILE_BACKED_VIRTUAL: u32 = 15;

#[repr(C)] struct StoragePropertyQuery { property_id: u32, query_type: u32, additional_parameters: [u8; 1], _pad: [u8; 3] }
#[repr(C)] struct StorageAdapterDescriptor { version: u32, size: u32, maximum_transfer_length: u32, maximum_physical_pages: u32, alignment_mask: u32, device_uses_down_port: u8, _pad: [u8; 3], bus_type: u32 }
#[repr(C)] struct StorageSeekPenaltyDescriptor { version: u32, size: u32, incurs_seek_penalty: u8, _pad: [u8; 3] }

#[cfg(windows)]
pub fn detect_media_type(root: &str) -> MediaType {
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::core::HSTRING;

    let volume_path = get_volume_path(root);
    let root_h = HSTRING::from(volume_path.as_str());
    let handle = match unsafe { CreateFileW(&root_h, GENERIC_READ.0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, None) } {
        Ok(h) => h,
        Err(e) => { warn!("detect_media_type: CreateFileW failed for {}: {:?}", volume_path, e); return MediaType::Unknown; }
    };
    if handle == INVALID_HANDLE_VALUE { warn!("detect_media_type: INVALID_HANDLE_VALUE for {}", volume_path); return MediaType::Unknown; }

    let bus_type = query_bus_type(handle);
    let seek_penalty = query_seek_penalty(handle);
    let _ = unsafe { CloseHandle(handle) };

    debug!("detect_media_type for {}: bus_type={:?}, seek_penalty={:?}", volume_path, bus_type, seek_penalty);
    match bus_type {
        Some(BUS_TYPE_USB) => MediaType::Usb,
        Some(BUS_TYPE_VIRTUAL) | Some(BUS_TYPE_FILE_BACKED_VIRTUAL) => MediaType::Vhd,
        _ => match seek_penalty { Some(true) => MediaType::Hdd, Some(false) => MediaType::Ssd, None => MediaType::Unknown },
    }
}

#[cfg(not(windows))]
pub fn detect_media_type(_root: &str) -> MediaType { MediaType::Unknown }

#[cfg(windows)]
pub fn is_subst_drive(root: &str) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::QueryDosDeviceW;
    use windows::core::PCWSTR;

    let drive = format!("{}:", root.trim_end_matches('\\').trim_end_matches(':'));
    let drive_wide: Vec<u16> = std::ffi::OsStr::new(&drive).encode_wide().chain(std::iter::once(0)).collect();
    let mut buffer = vec![0u16; 256];
    let len = unsafe { QueryDosDeviceW(PCWSTR(drive_wide.as_ptr()), Some(&mut buffer)) };
    if len == 0 { debug!("is_subst_drive: QueryDosDeviceW failed for {}", drive); return false; }
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    debug!("is_subst_drive: {} -> {}", drive, path);
    let parts: Vec<&str> = path.split('\\').filter(|s| !s.is_empty()).collect();
    parts.len() > 2
}

#[cfg(not(windows))]
pub fn is_subst_drive(_root: &str) -> bool { false }

pub fn get_volume_path(root: &str) -> String {
    let trimmed = root.trim_end_matches('\\');
    if trimmed.len() == 2 && trimmed.chars().nth(1) == Some(':') { format!(r"\\.\{}", trimmed) } else { trimmed.to_string() }
}

#[cfg(windows)]
fn query_bus_type(handle: windows::Win32::Foundation::HANDLE) -> Option<u32> {
    use windows::Win32::System::IO::DeviceIoControl;
    let query = StoragePropertyQuery { property_id: STORAGE_ADAPTER_PROPERTY, query_type: PROPERTY_STANDARD_QUERY, additional_parameters: [0], _pad: [0; 3] };
    let mut output = StorageAdapterDescriptor { version: 0, size: 0, maximum_transfer_length: 0, maximum_physical_pages: 0, alignment_mask: 0, device_uses_down_port: 0, _pad: [0; 3], bus_type: 0 };
    let mut bytes_returned = 0u32;
    match unsafe { DeviceIoControl(handle, IOCTL_STORAGE_QUERY_PROPERTY, Some(&query as *const _ as *const _), std::mem::size_of::<StoragePropertyQuery>() as u32, Some(&mut output as *mut _ as *mut _), std::mem::size_of::<StorageAdapterDescriptor>() as u32, Some(&mut bytes_returned), None) } {
        Ok(_) => { debug!("query_bus_type: bus_type={}", output.bus_type); Some(output.bus_type) }
        Err(e) => { debug!("query_bus_type failed: {:?}", e); None }
    }
}

#[cfg(windows)]
fn query_seek_penalty(handle: windows::Win32::Foundation::HANDLE) -> Option<bool> {
    use windows::Win32::System::IO::DeviceIoControl;
    let query = StoragePropertyQuery { property_id: STORAGE_DEVICE_SEEK_PENALTY_PROPERTY, query_type: PROPERTY_STANDARD_QUERY, additional_parameters: [0], _pad: [0; 3] };
    let mut output = StorageSeekPenaltyDescriptor { version: 0, size: 0, incurs_seek_penalty: 0, _pad: [0; 3] };
    let mut bytes_returned = 0u32;
    match unsafe { DeviceIoControl(handle, IOCTL_STORAGE_QUERY_PROPERTY, Some(&query as *const _ as *const _), std::mem::size_of::<StoragePropertyQuery>() as u32, Some(&mut output as *mut _ as *mut _), std::mem::size_of::<StorageSeekPenaltyDescriptor>() as u32, Some(&mut bytes_returned), None) } {
        Ok(_) => { let penalty = output.incurs_seek_penalty != 0; debug!("query_seek_penalty: incurs_seek_penalty={}", penalty); Some(penalty) }
        Err(e) => { debug!("query_seek_penalty failed: {:?}", e); None }
    }
}

