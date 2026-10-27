//! A narrow, noninteractive Keychain item; no process-wide interaction toggle.
use anyhow::{ensure, Context, Result};
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    boolean::CFBoolean,
    data::CFData,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
use security_framework::os::macos::keychain::SecKeychain;
use security_framework_sys::{
    item::*,
    keychain_item::{SecItemAdd, SecItemCopyMatching},
};
use std::{fs, path::Path};
use zeroize::Zeroizing;
unsafe extern "C" {
    static kSecUseAuthenticationUIFail: CFStringRef;
}

pub fn load(
    root: &Path,
    account: &str,
    required: bool,
    create: impl FnOnce() -> Zeroizing<[u8; 32]>,
) -> Result<Zeroizing<[u8; 32]>> {
    let keychain = if let Some(path) = std::env::var_os("AIPASS_PROXY_RECOVERY_KEYCHAIN") {
        // The override can address only a uniquely named disposable HOME. It
        // cannot redirect production recovery to an arbitrary keychain.
        let home = root
            .parent()
            .and_then(Path::parent)
            .context("invalid recovery root")?;
        ensure!(
            home.file_name().is_some_and(|n| n == "home")
                && home
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|n| n.to_string_lossy().starts_with("aipass-")),
            "recovery keychain override requires an isolated fixture HOME"
        );
        let path = fs::canonicalize(path)?;
        ensure!(
            path == fs::canonicalize(root)?.join("fixture.keychain-db"),
            "recovery keychain is outside fixture"
        );
        SecKeychain::open(path)?
    } else {
        SecKeychain::default().context("default Keychain unavailable; unlock Keychain and retry")?
    };
    unsafe {
        let key = |v| CFString::wrap_under_get_rule(v).into_CFType();
        let mut pairs = vec![
            (key(kSecClass), key(kSecClassGenericPassword)),
            (
                key(kSecAttrService),
                CFString::new("dev.aipass.proxy-uninstall.v1").into_CFType(),
            ),
            (key(kSecAttrAccount), CFString::new(account).into_CFType()),
            (
                key(kSecUseAuthenticationUI),
                key(kSecUseAuthenticationUIFail),
            ),
        ];
        let mut query = pairs.clone();
        query.push((
            key(kSecMatchSearchList),
            CFArray::from_CFTypes(std::slice::from_ref(&keychain)).into_CFType(),
        ));
        query.push((key(kSecReturnData), CFBoolean::true_value().into_CFType()));
        let query = CFDictionary::from_CFType_pairs(&query);
        let mut result = std::ptr::null();
        let status = SecItemCopyMatching(query.as_concrete_TypeRef(), &mut result);
        if status == -25300 {
            ensure!(
                !required,
                "proxy recovery device key is missing; uninstall stopped"
            );
            let value = create();
            pairs.push((key(kSecUseKeychain), keychain.into_CFType()));
            pairs.push((
                key(kSecValueData),
                CFData::from_buffer(value.as_slice()).into_CFType(),
            ));
            let query = CFDictionary::from_CFType_pairs(&pairs);
            ensure!(
                SecItemAdd(query.as_concrete_TypeRef(), std::ptr::null_mut()) == 0,
                "proxy recovery Keychain write denied; no configuration was changed"
            );
            return Ok(value);
        }
        ensure!(
            status == 0 && !result.is_null(),
            "proxy recovery Keychain read denied; unlock Keychain and retry"
        );
        let data = CFType::wrap_under_create_rule(result)
            .downcast::<CFData>()
            .context("invalid proxy recovery key data")?;
        ensure!(data.len() == 32, "invalid proxy recovery key length");
        let mut value = Zeroizing::new([0; 32]);
        value.copy_from_slice(data.bytes());
        Ok(value)
    }
}
