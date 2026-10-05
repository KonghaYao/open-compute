use super::*;

#[test]
fn snapshot_crypto_bounds_nonce_tamper_and_cross_domain_fail_closed() {
    let key = SecretBytes::new(vec![7; 32]);
    let crypto = SecretCrypto::new(&key, &hex::encode(Sha256::digest(key.expose()))).unwrap();
    let instance = InstanceId::generate();
    let worker = WorkerId::generate();
    let version = VersionId::generate();
    let identity = [8; 32];
    let snapshot = SecretBytes::new(b"snapshot secret bytes".to_vec());
    let sealed = crypto
        .seal_python_snapshot(&snapshot, instance, worker, version, &identity)
        .unwrap();
    let second = crypto
        .seal_python_snapshot(&snapshot, instance, worker, version, &identity)
        .unwrap();
    assert_ne!(sealed.nonce, second.nonce);
    assert_eq!(
        crypto
            .open_python_snapshot(&sealed, instance, worker, version, &identity)
            .unwrap()
            .expose(),
        snapshot.expose()
    );
    for (instance, worker, version, identity) in [
        (InstanceId::generate(), worker, version, identity),
        (instance, WorkerId::generate(), version, identity),
        (instance, worker, VersionId::generate(), identity),
        (instance, worker, version, [9; 32]),
    ] {
        assert_eq!(
            crypto
                .open_python_snapshot(&sealed, instance, worker, version, &identity)
                .unwrap_err()
                .code(),
            ErrorCode::VersionInvariantViolation
        );
    }
    let mut tampered = sealed.clone();
    tampered.ciphertext[0] ^= 1;
    assert!(
        crypto
            .open_python_snapshot(&tampered, instance, worker, version, &identity)
            .is_err()
    );
    let mut tampered = sealed.clone();
    tampered.nonce[0] ^= 1;
    assert!(
        crypto
            .open_python_snapshot(&tampered, instance, worker, version, &identity)
            .is_err()
    );
    for malformed in [
        SecretEnvelope {
            version: 0,
            ..sealed.clone()
        },
        SecretEnvelope {
            algorithm: "other".to_owned(),
            ..sealed.clone()
        },
        SecretEnvelope {
            key_id: "0".repeat(64),
            ..sealed.clone()
        },
        SecretEnvelope {
            nonce: vec![0; 23],
            ..sealed.clone()
        },
        SecretEnvelope {
            ciphertext: vec![0; 16],
            ..sealed.clone()
        },
    ] {
        assert!(
            crypto
                .open_python_snapshot(&malformed, instance, worker, version, &identity)
                .is_err()
        );
    }
    assert!(
        crypto
            .seal_python_snapshot(
                &SecretBytes::new(Vec::new()),
                instance,
                worker,
                version,
                &identity
            )
            .is_err()
    );
    let over_limit = SecretBytes::new(vec![0; MAX_PYTHON_SNAPSHOT_BYTES + 1]);
    assert!(
        crypto
            .seal_python_snapshot(&over_limit, instance, worker, version, &identity)
            .is_err()
    );
    drop(over_limit);
    let oversized = SecretEnvelope {
        ciphertext: vec![0; MAX_PYTHON_SNAPSHOT_BYTES + 17],
        ..sealed.clone()
    };
    assert!(
        crypto
            .open_python_snapshot(&oversized, instance, worker, version, &identity)
            .is_err()
    );
    assert!(
        crypto
            .decrypt(&sealed, instance, worker, version, "snapshot", "revision")
            .is_err()
    );
    let env = crypto
        .encrypt(&snapshot, instance, worker, version, "snapshot", "revision")
        .unwrap();
    assert!(
        crypto
            .open_python_snapshot(&env, instance, worker, version, &identity)
            .is_err()
    );
}
