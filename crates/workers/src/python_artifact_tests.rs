use super::*;

fn fixture() -> (PreparedPythonIdentity, SecretCrypto, SecretBytes) {
    let key = SecretBytes::new(vec![7; 32]);
    let crypto = SecretCrypto::new(&key, &hex::encode(Sha256::digest(key.expose()))).unwrap();
    let identity = PreparedPythonIdentity {
        schema_version: 1,
        instance_id: InstanceId::generate(),
        worker_id: WorkerId::generate(),
        version_id: VersionId::generate(),
        worker_code_sha256: "a".repeat(64),
        runtime: PythonRuntimeIdentity {
            pin: PythonRuntimePin {
                workerd_revision: "b".repeat(40),
                workerd_binary_sha256: "c".repeat(64),
                process_flags: vec!["--experimental".to_owned()],
                pyodide_bundle_sha256: "d".repeat(64),
                runtime_assets_sha256: "e".repeat(64),
            },
            module_inventory_sha256: "f".repeat(64),
        },
    };
    (
        identity,
        crypto,
        SecretBytes::new(b"snapshot secret bytes".to_vec()),
    )
}

#[test]
fn encrypted_snapshot_survives_persistence_and_key_reload_without_plaintext() {
    let (identity, crypto, snapshot) = fixture();
    let artifact = EncryptedPythonArtifact::seal(identity.clone(), &snapshot, &crypto).unwrap();
    let second = EncryptedPythonArtifact::seal(identity.clone(), &snapshot, &crypto).unwrap();
    assert_ne!(artifact.bytes(), second.bytes());
    assert!(
        !artifact
            .bytes()
            .windows(snapshot.expose().len())
            .any(|part| part == snapshot.expose())
    );
    assert!(!format!("{artifact:?}").contains("secret"));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("encrypted-artifact");
    std::fs::write(&path, artifact.bytes()).unwrap();
    drop(crypto);
    let key = SecretBytes::new(vec![7; 32]);
    let reloaded = SecretCrypto::new(&key, &hex::encode(Sha256::digest(key.expose()))).unwrap();
    let restored =
        EncryptedPythonArtifact::open(&std::fs::read(&path).unwrap(), &identity, &reloaded)
            .unwrap();
    assert_eq!(restored.expose(), snapshot.expose());
}

#[test]
fn each_source_runtime_and_environment_input_changes_identity_and_rejects_restore() {
    let (identity, crypto, snapshot) = fixture();
    let artifact = EncryptedPythonArtifact::seal(identity.clone(), &snapshot, &crypto).unwrap();
    let digest = identity.sha256().unwrap();
    let mut changes = Vec::new();
    macro_rules! changed {
        ($field:ident, $value:expr) => {{
            let mut next = identity.clone();
            next.$field = $value;
            changes.push(next);
        }};
        (pin.$field:ident, $value:expr) => {{
            let mut next = identity.clone();
            next.runtime.pin.$field = $value;
            changes.push(next);
        }};
        (runtime.$field:ident, $value:expr) => {{
            let mut next = identity.clone();
            next.runtime.$field = $value;
            changes.push(next);
        }};
    }
    changed!(instance_id, InstanceId::generate());
    changed!(worker_id, WorkerId::generate());
    changed!(version_id, VersionId::generate());
    changed!(worker_code_sha256, "2".repeat(64));
    changed!(pin.workerd_revision, "2".repeat(40));
    changed!(pin.workerd_binary_sha256, "2".repeat(64));
    changed!(pin.process_flags, vec!["--another-mode".to_owned()]);
    changed!(pin.pyodide_bundle_sha256, "2".repeat(64));
    changed!(pin.runtime_assets_sha256, "2".repeat(64));
    changed!(runtime.module_inventory_sha256, "2".repeat(64));
    for next in changes {
        assert_ne!(next.sha256().unwrap(), digest);
        assert_eq!(
            EncryptedPythonArtifact::open(artifact.bytes(), &next, &crypto)
                .unwrap_err()
                .code(),
            ErrorCode::VersionInvariantViolation
        );
    }
}

fn rewrite_header(bytes: &[u8], change: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let size = u32::from_be_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut value: serde_json::Value = serde_json::from_slice(&bytes[12..12 + size]).unwrap();
    change(&mut value);
    // Serialize in the product's field order unless intentionally testing an
    // invalid schema; generic Value uses map order and is not canonical here.
    let header = serde_json::from_value::<Header>(value.clone())
        .ok()
        .map_or_else(
            || serde_json::to_vec(&value).unwrap(),
            |header| serde_json::to_vec(&header).unwrap(),
        );
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&(header.len() as u32).to_be_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&bytes[12 + size..]);
    out
}

#[test]
fn artifact_rejects_truncation_corruption_metadata_and_noncanonical_headers() {
    let (identity, crypto, snapshot) = fixture();
    let artifact = EncryptedPythonArtifact::seal(identity.clone(), &snapshot, &crypto).unwrap();
    let bytes = artifact.bytes();
    let size = u32::from_be_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut invalid = vec![
        Vec::new(),
        bytes[..11].to_vec(),
        bytes[..12].to_vec(),
        bytes[..12 + size - 1].to_vec(),
        bytes[..bytes.len() - 1].to_vec(),
    ];
    for index in [0, 8, 12 + size] {
        let mut corrupt = bytes.to_vec();
        corrupt[index] ^= 1;
        invalid.push(corrupt);
    }
    for header_size in [0_u32, MAX_HEADER_BYTES as u32 + 1] {
        let mut corrupt = bytes.to_vec();
        corrupt[8..12].copy_from_slice(&header_size.to_be_bytes());
        invalid.push(corrupt);
    }
    let mut trailing = bytes.to_vec();
    trailing.push(0);
    invalid.push(trailing);
    for (field, value) in [
        ("snapshotSize", serde_json::json!(0)),
        ("snapshotSize", serde_json::json!(15)),
        (
            "snapshotSize",
            serde_json::json!(MAX_PYTHON_SNAPSHOT_BYTES + 1),
        ),
        (
            "snapshotSize",
            serde_json::json!(snapshot.expose().len() + 1),
        ),
        ("snapshotSha256", serde_json::json!("A".repeat(64))),
        ("snapshotSha256", serde_json::json!("0".repeat(64))),
        ("unknown", serde_json::json!(true)),
    ] {
        invalid.push(rewrite_header(bytes, |header| header[field] = value));
    }
    for (field, value) in [
        ("version", serde_json::json!(0)),
        ("algorithm", serde_json::json!("other")),
        ("key_id", serde_json::json!("0".repeat(64))),
        ("nonce", serde_json::json!([])),
        ("ciphertext", serde_json::json!([1])),
    ] {
        invalid.push(rewrite_header(bytes, |header| {
            header["envelope"][field] = value;
        }));
    }
    let header: Header = serde_json::from_slice(&bytes[12..12 + size]).unwrap();
    let pretty = serde_json::to_vec_pretty(&header).unwrap();
    let mut noncanonical = MAGIC.to_vec();
    noncanonical.extend_from_slice(&(pretty.len() as u32).to_be_bytes());
    noncanonical.extend_from_slice(&pretty);
    noncanonical.extend_from_slice(&bytes[12 + size..]);
    invalid.push(noncanonical);
    for bytes in invalid {
        let error = EncryptedPythonArtifact::open(&bytes, &identity, &crypto).unwrap_err();
        assert_eq!(error.code(), ErrorCode::VersionInvariantViolation);
        assert!(!error.to_string().contains("snapshot secret"));
    }
}

#[test]
fn malformed_identity_empty_snapshot_wrong_key_and_secret_domain_are_rejected() {
    let (identity, crypto, snapshot) = fixture();
    let artifact = EncryptedPythonArtifact::seal(identity.clone(), &snapshot, &crypto).unwrap();
    let key = SecretBytes::new(vec![9; 32]);
    let other = SecretCrypto::new(&key, &hex::encode(Sha256::digest(key.expose()))).unwrap();
    assert!(EncryptedPythonArtifact::open(artifact.bytes(), &identity, &other).is_err());
    assert!(
        EncryptedPythonArtifact::seal(identity.clone(), &SecretBytes::new(Vec::new()), &crypto)
            .is_err()
    );
    let mut malformed = identity.clone();
    malformed.schema_version = 0;
    assert!(malformed.sha256().is_err());
    malformed = identity.clone();
    malformed.runtime.pin.workerd_revision = "B".repeat(40);
    assert!(malformed.sha256().is_err());
    malformed = identity.clone();
    malformed.worker_code_sha256 = "short".to_owned();
    assert!(malformed.sha256().is_err());
    for flags in [
        vec![],
        vec!["--".to_owned()],
        vec!["--token=secret".to_owned()],
        vec!["--some flag".to_owned()],
        vec!["--experimental".to_owned(); 2],
    ] {
        malformed = identity.clone();
        malformed.runtime.pin.process_flags = flags;
        assert!(malformed.sha256().is_err());
    }
    malformed = identity.clone();
    malformed.runtime.module_inventory_sha256 = "z".repeat(64);
    assert!(malformed.sha256().is_err());
    let digest = identity.sha256().unwrap();
    let sealed = crypto
        .seal_python_snapshot(
            &snapshot,
            identity.instance_id,
            identity.worker_id,
            identity.version_id,
            &digest,
        )
        .unwrap();
    assert!(
        crypto
            .decrypt(
                &sealed,
                identity.instance_id,
                identity.worker_id,
                identity.version_id,
                "snapshot",
                &hex::encode(digest)
            )
            .is_err()
    );
    let env_secret = crypto
        .encrypt(
            &snapshot,
            identity.instance_id,
            identity.worker_id,
            identity.version_id,
            "snapshot",
            &hex::encode(digest),
        )
        .unwrap();
    assert!(
        crypto
            .open_python_snapshot(
                &env_secret,
                identity.instance_id,
                identity.worker_id,
                identity.version_id,
                &digest
            )
            .is_err()
    );
    for (instance, worker, version, prepared) in [
        (
            InstanceId::generate(),
            identity.worker_id,
            identity.version_id,
            digest,
        ),
        (
            identity.instance_id,
            WorkerId::generate(),
            identity.version_id,
            digest,
        ),
        (
            identity.instance_id,
            identity.worker_id,
            VersionId::generate(),
            digest,
        ),
        (
            identity.instance_id,
            identity.worker_id,
            identity.version_id,
            [0; 32],
        ),
    ] {
        assert!(
            crypto
                .open_python_snapshot(&sealed, instance, worker, version, &prepared)
                .is_err()
        );
    }
}
