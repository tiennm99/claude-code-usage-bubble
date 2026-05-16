// Typed wrapper over a tiny subset of the Win32 registry API.
//
// All operations target `HKEY_CURRENT_USER` keys by default (the app only
// reads/writes user-scoped state — startup entry, theme detection, etc.).
// Each call opens and closes the key internally; there is no caching so
// state changes by other processes are visible immediately.

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

use super::string::to_utf16_nul;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("registry open failed for {key}: code {code}")]
    Open { key: String, code: u32 },
    #[error("registry write failed for {key}\\{value}: code {code}")]
    Write { key: String, value: String, code: u32 },
}

/// Read a `REG_DWORD` value under `HKEY_CURRENT_USER\<subkey>`.
/// Returns `None` if the key or value does not exist.
pub fn read_u32(subkey: &str, value_name: &str) -> Option<u32> {
    let subkey_w = to_utf16_nul(subkey);
    let value_w = to_utf16_nul(value_name);
    unsafe {
        let mut hkey = HKEY::default();
        let open = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(subkey_w.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        );
        if open != ERROR_SUCCESS {
            return None;
        }
        let mut data: u32 = 0;
        let mut size: u32 = std::mem::size_of::<u32>() as u32;
        let query = RegQueryValueExW(
            hkey,
            PCWSTR::from_raw(value_w.as_ptr()),
            None,
            None,
            Some((&mut data as *mut u32) as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        if query == ERROR_SUCCESS {
            Some(data)
        } else {
            None
        }
    }
}

/// Test whether a value (any type) exists under `HKEY_CURRENT_USER\<subkey>`.
pub fn value_exists(subkey: &str, value_name: &str) -> bool {
    let subkey_w = to_utf16_nul(subkey);
    let value_w = to_utf16_nul(value_name);
    unsafe {
        let mut hkey = HKEY::default();
        let open = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(subkey_w.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        );
        if open != ERROR_SUCCESS {
            return false;
        }
        let mut size: u32 = 0;
        let query = RegQueryValueExW(
            hkey,
            PCWSTR::from_raw(value_w.as_ptr()),
            None,
            None,
            None,
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        query == ERROR_SUCCESS
    }
}

/// Write a string value as `REG_SZ` under `HKEY_CURRENT_USER\<subkey>`.
pub fn write_string(subkey: &str, value_name: &str, value: &str) -> Result<(), RegistryError> {
    let subkey_w = to_utf16_nul(subkey);
    let value_w = to_utf16_nul(value_name);
    let data_w = to_utf16_nul(value);
    unsafe {
        let mut hkey = HKEY::default();
        let open = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(subkey_w.as_ptr()),
            0,
            KEY_WRITE,
            &mut hkey,
        );
        if open != ERROR_SUCCESS {
            return Err(RegistryError::Open {
                key: subkey.to_string(),
                code: open.0,
            });
        }
        let bytes = std::slice::from_raw_parts(
            data_w.as_ptr() as *const u8,
            data_w.len() * std::mem::size_of::<u16>(),
        );
        let res = RegSetValueExW(
            hkey,
            PCWSTR::from_raw(value_w.as_ptr()),
            0,
            REG_SZ,
            Some(bytes),
        );
        let _ = RegCloseKey(hkey);
        if res == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(RegistryError::Write {
                key: subkey.to_string(),
                value: value_name.to_string(),
                code: res.0,
            })
        }
    }
}

/// Delete a value under `HKEY_CURRENT_USER\<subkey>`. Returns `Ok(())` even
/// if the value never existed.
pub fn delete_value(subkey: &str, value_name: &str) -> Result<(), RegistryError> {
    let subkey_w = to_utf16_nul(subkey);
    let value_w = to_utf16_nul(value_name);
    unsafe {
        let mut hkey = HKEY::default();
        let open = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(subkey_w.as_ptr()),
            0,
            KEY_WRITE,
            &mut hkey,
        );
        if open != ERROR_SUCCESS {
            return Err(RegistryError::Open {
                key: subkey.to_string(),
                code: open.0,
            });
        }
        let _ = RegDeleteValueW(hkey, PCWSTR::from_raw(value_w.as_ptr()));
        let _ = RegCloseKey(hkey);
        Ok(())
    }
}
