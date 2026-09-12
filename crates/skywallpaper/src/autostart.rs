use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
};
use windows::core::{PCWSTR, w};

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: windows::core::PCWSTR = w!("SkyWallpaper");

fn run_command_line(path: &Path) -> Vec<u16> {
    let mut wide = Vec::new();
    wide.push(u16::from(b'"'));
    wide.extend(path.as_os_str().encode_wide());
    wide.push(u16::from(b'"'));
    wide.push(0);
    wide
}

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
            let wide = run_command_line(&path);
            let bytes: &[u8] =
                std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            RegSetValueExW(hkey, VALUE, None, REG_SZ, Some(bytes)).ok()?;
        } else {
            let _ = RegDeleteValueW(hkey, VALUE);
        }
        let _ = windows::Win32::System::Registry::RegCloseKey(hkey);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    #[test]
    fn run_command_line_quotes_paths_with_spaces() {
        let wide = run_command_line(Path::new(r"C:\Program Files\SkyWallpaper\skywallpaper.exe"));
        assert_eq!(*wide.last().unwrap(), 0);
        let text = OsString::from_wide(&wide[..wide.len() - 1]);
        assert_eq!(
            text.to_str().unwrap(),
            r#""C:\Program Files\SkyWallpaper\skywallpaper.exe""#
        );
    }
}
