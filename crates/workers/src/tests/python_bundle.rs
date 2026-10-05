use super::*;

#[test]
fn python_bundle_preserves_modules_and_rejects_invalid_source_and_main_types() {
    let limits = BundleLimits::default();
    let modules = vec![
        ModuleInput {
            name: "entry.py".to_owned(),
            module_type: ModuleType::Python,
            bytes: b"from helpers import message\n".to_vec(),
        },
        ModuleInput {
            name: "helpers.py".to_owned(),
            module_type: ModuleType::Python,
            bytes: "message = '你好'\n".as_bytes().to_vec(),
        },
        ModuleInput {
            name: "python_modules/example/data.bin".to_owned(),
            module_type: ModuleType::Data,
            bytes: vec![0, 255, 1],
        },
    ];
    let bundle = CanonicalBundle::build("entry.py", modules.clone(), limits).unwrap();
    let reparsed = CanonicalBundle::parse(bundle.bytes().to_vec(), limits).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bundle");
    fs::write(&path, bundle.bytes()).unwrap();
    let staged = StagedBundle::open(path.clone(), limits).unwrap();
    assert_eq!(staged.manifest(), reparsed.manifest());
    assert_eq!(staged.sha256(), reparsed.sha256());
    for input in &modules {
        let entry = reparsed
            .manifest()
            .modules
            .iter()
            .find(|entry| entry.name == input.name)
            .unwrap();
        assert_eq!(entry.module_type, input.module_type);
        assert_eq!(reparsed.module_bytes(entry).unwrap(), input.bytes);
    }
    assert_eq!(
        serde_json::to_string(&ModuleType::Python).unwrap(),
        "\"python\""
    );

    for (name, kind, source) in [
        (
            "entry.py",
            ModuleType::EsModule,
            b"export default {};".as_slice(),
        ),
        ("entry.js", ModuleType::Python, b"pass".as_slice()),
        ("entry.py", ModuleType::Python, &[255]),
    ] {
        assert_eq!(
            CanonicalBundle::build(
                name,
                vec![ModuleInput {
                    name: name.to_owned(),
                    module_type: kind,
                    bytes: source.to_vec(),
                }],
                limits,
            )
            .unwrap_err()
            .code(),
            ErrorCode::BundleInvalid
        );
    }
    let corrupt = rewrite_bundle(bundle.bytes(), |manifest, blob| {
        let entry = manifest
            .modules
            .iter_mut()
            .find(|entry| entry.name == "entry.py")
            .unwrap();
        let start = entry.offset as usize;
        blob[start] = 255;
        entry.sha256 = hex::encode(sha2::Sha256::digest(
            &blob[start..start + entry.size as usize],
        ));
    });
    assert_eq!(
        CanonicalBundle::parse(corrupt.clone(), limits)
            .unwrap_err()
            .code(),
        ErrorCode::BundleInvalid
    );
    fs::write(&path, corrupt).unwrap();
    assert_eq!(
        StagedBundle::open(path, limits).unwrap_err().code(),
        ErrorCode::BundleInvalid
    );
}
