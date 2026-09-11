use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, KEY_SET_VALUE, KEY_QUERY_VALUE, REG_SZ, RegCreateKeyExW, RegDeleteValueW,
    RegSetValueExW, REG_OPTION_NON_VOLATILE,
};
use windows::core::{w, PCWSTR};

const RUN_KEY: windows::core::PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: windows::core::PCWSTR = w!("SkyWallpaper");

pub fn set_enabled(enable: bool) -> anyhow::Result<()> {
    unsafe {
        let mut hkey = Default::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            None,
            &mut hkey,
            None,
        )
        .ok()?;
        if enable {
            let path = std::env::current_exe()?;
            let wide: Vec<u16> = path
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let bytes: &[u8] = std::slice::from_raw_parts(
                wide.as_ptr() as *const u8,
                wide.len() * 2,
            );
            RegSetValueExW(hkey, VALUE, None, REG_SZ, Some(bytes)).ok()?;
        } else {
            let _ = RegDeleteValueW(hkey, VALUE);
        }
        let _ = windows::Win32::System::Registry::RegCloseKey(hkey);
    }
    Ok(())
}
