#[cfg(windows)]
use anyhow::ensure;
use anyhow::Result;
use std::path::Path;
use zeroize::Zeroizing;

#[cfg(target_os = "macos")]
pub(super) fn load(root: &Path, required: bool) -> Result<Zeroizing<[u8; 32]>> {
    let account = crate::namespace_for_vault_dir(root)?;
    aipass_config_writers::proxy_recovery::macos_key::load(root, &account, required, random_key)
}

#[cfg(windows)]
pub(super) fn load(root: &Path, required: bool) -> Result<Zeroizing<[u8; 32]>> {
    use aipass_config_writers::native_auth::Resource;
    let file = Resource::File(root.join("device-key.dpapi"));
    let bytes = match file.read()? {
        Some(bytes) => protect(&bytes, false)?,
        None => {
            ensure!(
                !required,
                "proxy recovery device key is missing; uninstall stopped"
            );
            let key = random_key();
            file.write(Some(&protect(key.as_slice(), true)?))?;
            Zeroizing::new(key.to_vec())
        }
    };
    ensure!(bytes.len() == 32, "invalid proxy recovery device key");
    let mut key = Zeroizing::new([0; 32]);
    key.copy_from_slice(&bytes);
    Ok(key)
}

#[cfg(any(windows, target_os = "macos"))]
fn random_key() -> Zeroizing<[u8; 32]> {
    use rand_core::{OsRng, RngCore};
    let mut key = Zeroizing::new([0; 32]);
    OsRng.fill_bytes(key.as_mut());
    key
}

#[cfg(windows)]
fn protect(bytes: &[u8], encrypt: bool) -> Result<Zeroizing<Vec<u8>>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into()?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    ensure!(
        ok != 0,
        "proxy recovery device protection failed; uninstall stopped"
    );
    let mut result = Zeroizing::new(Vec::new());
    if !output.pbData.is_null() {
        let slice =
            unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
        result.extend_from_slice(slice);
        use zeroize::Zeroize;
        slice.zeroize();
        unsafe {
            LocalFree(output.pbData.cast());
        }
    }
    Ok(result)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(super) fn load(_: &Path, _: bool) -> Result<Zeroizing<[u8; 32]>> {
    anyhow::bail!("proxy recovery device storage is unsupported")
}
