use super::*;

#[test]
fn corrupt_secondary_backup_changes_no_targets() {
    let root = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let targets: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|name| root.path().join(name))
        .collect();
    let writes: Vec<_> = targets
        .iter()
        .enumerate()
        .map(|(index, path)| PreparedWrite {
            target_path: path.clone(),
            backup_path: root.path().join(format!("{id}-{index}.aipbackup")),
            content: b"new".to_vec(),
            original: b"original".to_vec(),
            target_existed: true,
        })
        .collect();
    for path in &targets {
        fs::write(path, b"new").unwrap();
    }
    write_encrypted_backups(id, &writes, &[7; KEY_LEN]).unwrap();
    let mut backup: EncryptedBackup = read_json(&writes[1].backup_path).unwrap();
    backup.ciphertext = encrypt_bytes(
        &[8; KEY_LEN],
        backup_aad(id, &backup.target_path).as_bytes(),
        b"original",
    )
    .unwrap();
    write_json(&writes[1].backup_path, &backup).unwrap();
    assert!(rollback_encrypted(&writes[0].backup_path, &[7; KEY_LEN]).is_err());
    for path in targets {
        assert_eq!(fs::read(path).unwrap(), b"new");
    }
}

#[test]
fn prepared_rollback_rejects_external_changes_and_restores_absence() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("settings.json");
    let id = Uuid::new_v4();
    let write = PreparedWrite {
        target_path: target.clone(),
        backup_path: root.path().join(format!("{id}-main.aipbackup")),
        content: b"new".to_vec(),
        original: Vec::new(),
        target_existed: false,
    };
    fs::write(&target, b"new").unwrap();
    write_encrypted_backups(id, std::slice::from_ref(&write), &[7; KEY_LEN]).unwrap();
    let prepared = prepare_rollback_encrypted(&write.backup_path, &[7; KEY_LEN]).unwrap();
    fs::write(&target, b"external edit").unwrap();
    assert!(prepared.apply().is_err());
    assert_eq!(fs::read(&target).unwrap(), b"external edit");
    rollback_encrypted(&write.backup_path, &[7; KEY_LEN]).unwrap();
    assert!(!target.exists());
}
