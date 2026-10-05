//! Reviewed static cf upload input; this module never builds or patches SDK code.

use super::*;
use bytes::Bytes;
use futures::stream;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs;

const MAX_UPLOAD_BYTES: usize = 34 * 1024 * 1024;
const BOUNDARY: &str = "open-compute-python-main-test-boundary";

#[derive(Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Module {
    name: String,
    upload_name: String,
    mime: String,
    size: usize,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    cf: String,
    cli_sha256: String,
    build_output_config_sha256: String,
    producer: String,
    content_type: String,
    size: usize,
    sha256: String,
    metadata: Value,
    modules: Vec<Module>,
}

pub(crate) struct Capture {
    pub(crate) metadata: Value,
    modules: BTreeMap<String, (String, Vec<u8>)>,
    pub(crate) sha256: String,
}

pub(crate) struct Bindings<'a> {
    pub(crate) kv: &'a str,
    pub(crate) d1: &'a str,
    pub(crate) r2: &'a str,
    pub(crate) outbound: &'a str,
    pub(crate) revision: &'a str,
    pub(crate) secret: &'a str,
}

impl Bindings<'_> {
    pub(crate) fn metadata(&self) -> Vec<Value> {
        vec![
            json!({"name":"KV", "type":"kv_namespace", "namespace_id":self.kv}),
            json!({"name":"DB", "type":"d1", "id":self.d1}),
            json!({"name":"BUCKET", "type":"r2_bucket", "bucket_name":self.r2}),
            json!({"name":"REVISION", "type":"plain_text", "text":self.revision}),
            json!({"name":"TOKEN", "type":"secret_text", "text":self.secret}),
            json!({"name":"OUTBOUND_URL", "type":"plain_text", "text":self.outbound}),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UploadFault {
    WrongMainMime,
    MissingMain,
    UnknownFlag,
    DuplicateModule,
    ReservedModule,
    MainSyntax,
    PackageSyntax,
    ImportException,
    PackageMissing,
    PackageDataMissing,
    PackageDataCorrupt,
}

impl Capture {
    pub(crate) async fn load(input: &str, source: &str, package_files: &[&str]) -> Self {
        let directory = repo_root().join(input);
        let manifest_path = directory.join("manifest.json");
        let manifest_bytes = fs::read(&manifest_path).expect(
            "Missing reviewed official cf Python Build Output capture. Generate with the official builder and prepare-python-main.ts; synthetic SDK input is forbidden.",
        );
        let mut manifest: Manifest = serde_json::from_slice(&manifest_bytes).unwrap();
        let package: Value =
            serde_json::from_slice(&fs::read(repo_root().join("package.json")).unwrap()).unwrap();
        assert_eq!(manifest.cf, package["catalog"]["cf"].as_str().unwrap());
        assert_eq!(manifest.producer, "cf workers versions create --prebuilt");
        for hash in [
            &manifest.sha256,
            &manifest.cli_sha256,
            &manifest.build_output_config_sha256,
        ] {
            assert!(
                hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            );
        }
        let file = directory.join("upload.multipart");
        assert!(fs::metadata(&file).unwrap().len() <= MAX_UPLOAD_BYTES as u64);
        let bytes = fs::read(file).unwrap();
        assert_eq!(bytes.len(), manifest.size);
        assert_eq!(hex::encode(Sha256::digest(&bytes)), manifest.sha256);
        let boundary = multer::parse_boundary(&manifest.content_type).unwrap();
        let mut multipart = multer::Multipart::new(
            stream::once(async move { Ok::<_, std::io::Error>(Bytes::from(bytes)) }),
            boundary,
        );
        let mut modules = BTreeMap::new();
        let mut metadata = None;
        let mut inventory = Vec::new();
        while let Some(field) = multipart.next_field().await.unwrap() {
            let name = field.name().unwrap().to_owned();
            if name == "metadata" {
                assert!(metadata.is_none(), "duplicate metadata");
                metadata =
                    Some(serde_json::from_slice::<Value>(&field.bytes().await.unwrap()).unwrap());
                continue;
            }
            assert_eq!(field.file_name(), Some(name.as_str()));
            let canonical = name.strip_prefix("./").unwrap_or(&name).to_owned();
            assert!(
                canonical
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
            );
            assert!(!name.contains(['"', '\r', '\n', '\\']));
            let mime = field.content_type().unwrap().to_string();
            let bytes = field.bytes().await.unwrap().to_vec();
            inventory.push(Module {
                name: canonical,
                upload_name: name.clone(),
                mime: mime.clone(),
                size: bytes.len(),
                sha256: hex::encode(Sha256::digest(&bytes)),
            });
            assert!(modules.insert(name, (mime, bytes)).is_none());
        }
        assert_eq!(metadata.as_ref(), Some(&manifest.metadata));
        inventory.sort_by(|a, b| a.name.cmp(&b.name));
        manifest.modules.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(inventory, manifest.modules);
        assert!(
            inventory
                .windows(2)
                .all(|pair| pair[0].name != pair[1].name)
        );
        assert_eq!(manifest.metadata["compatibility_date"], "2026-09-08");
        assert_eq!(
            manifest.metadata["compatibility_flags"],
            json!([
                "python_workers",
                "enable_python_external_sdk",
                "python_dedicated_snapshot"
            ])
        );
        let sdk: Value = serde_json::from_slice(
            &fs::read(repo_root().join("test/fixtures/python-main/sdk-inventory.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(sdk["package"], "workers-runtime-sdk");
        assert_eq!(sdk["version"], "1.9.2");
        assert_eq!(
            sdk["wheelSha256"],
            "f928e20afddaaf01f216b3ced8884ca1e7675681bb44692add753317fc9ce89d"
        );
        let lock: toml::Value = toml::from_str(
            &fs::read_to_string(directory.join("pylock.toml"))
                .expect("reviewed Python input must retain its official package lock"),
        )
        .unwrap();
        assert_eq!(lock["lock-version"].as_str(), Some("1.0"));
        let packages = lock["packages"].as_array().unwrap();
        let sdk_packages: Vec<_> = packages
            .iter()
            .filter(|package| package["name"].as_str() == sdk["package"].as_str())
            .collect();
        assert_eq!(sdk_packages.len(), 1);
        assert_eq!(sdk_packages[0]["version"].as_str(), sdk["version"].as_str());
        let wheels = sdk_packages[0]["wheels"].as_array().unwrap();
        assert_eq!(wheels.len(), 1);
        assert_eq!(
            wheels[0]["hashes"]["sha256"].as_str(),
            sdk["wheelSha256"].as_str()
        );
        let expected = sdk["modules"].as_array().unwrap();
        assert_eq!(expected.len(), 19);
        for expected in expected {
            let module = inventory
                .iter()
                .find(|module| module.name == expected["name"].as_str().unwrap())
                .unwrap();
            assert_eq!(module.mime, expected["mime"]);
            assert_eq!(module.size, expected["size"].as_u64().unwrap() as usize);
            assert_eq!(module.sha256, expected["sha256"]);
        }
        let main = manifest.metadata["main_module"].as_str().unwrap();
        assert!(
            inventory
                .iter()
                .any(|m| m.name == main && m.mime == "text/x-python")
        );
        for name in std::iter::once(main).chain(package_files.iter().copied()) {
            let bytes = &modules
                .iter()
                .find(|(wire, _)| wire.strip_prefix("./").unwrap_or(wire) == name)
                .expect("captured application module must exist")
                .1
                .1;
            assert_eq!(
                bytes,
                &fs::read(repo_root().join(source).join(name)).unwrap()
            );
        }
        Self {
            metadata: manifest.metadata,
            modules,
            sha256: manifest.sha256,
        }
    }

    pub(crate) fn render(&self, values: &[Value], fault: Option<UploadFault>) -> Vec<u8> {
        let mut metadata = self.metadata.clone();
        let bindings = metadata["bindings"].as_array_mut().unwrap();
        assert_eq!(bindings.len(), values.len());
        for value in values {
            let matching = bindings
                .iter_mut()
                .filter(|binding| binding["name"] == value["name"])
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1);
            let matching = matching.into_iter().next().unwrap();
            assert_eq!(matching["type"], value["type"]);
            *matching = value.clone();
        }
        if fault == Some(UploadFault::UnknownFlag) {
            metadata["compatibility_flags"]
                .as_array_mut()
                .unwrap()
                .push(json!("unknown_python_flag"));
        }
        let mut output = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n").into_bytes();
        output.extend_from_slice(&serde_json::to_vec(&metadata).unwrap());
        output.extend_from_slice(b"\r\n");
        let import_exception = format!("raise RuntimeError({:?})\n", PYTHON_SECRETS[0]);
        for (name, (mime, bytes)) in &self.modules {
            let canonical = name.strip_prefix("./").unwrap_or(name);
            let main = canonical == metadata["main_module"].as_str().unwrap();
            if main && fault == Some(UploadFault::MissingMain) {
                continue;
            }
            if (canonical.ends_with("greeting/__init__.py")
                && fault == Some(UploadFault::PackageMissing))
                || (canonical.ends_with("greeting/message.json")
                    && fault == Some(UploadFault::PackageDataMissing))
            {
                continue;
            }
            let bytes = if (main && fault == Some(UploadFault::MainSyntax))
                || (canonical.ends_with("greeting/__init__.py")
                    && fault == Some(UploadFault::PackageSyntax))
            {
                b"this is invalid Python syntax !!!\n".as_slice()
            } else if main && fault == Some(UploadFault::ImportException) {
                import_exception.as_bytes()
            } else if canonical.ends_with("greeting/message.json")
                && fault == Some(UploadFault::PackageDataCorrupt)
            {
                b"{invalid package JSON".as_slice()
            } else {
                bytes.as_slice()
            };
            assert!(
                !bytes
                    .windows(BOUNDARY.len())
                    .any(|part| part == BOUNDARY.as_bytes())
            );
            let mime = if main && fault == Some(UploadFault::WrongMainMime) {
                "text/plain"
            } else {
                mime
            };
            let copies = if main && fault == Some(UploadFault::DuplicateModule) {
                2
            } else {
                1
            };
            for _ in 0..copies {
                output.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{name}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes());
                output.extend_from_slice(bytes);
                output.extend_from_slice(b"\r\n");
            }
        }
        if fault == Some(UploadFault::ReservedModule) {
            output.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"cloudflare-internal:tenant\"; filename=\"cloudflare-internal:tenant\"\r\nContent-Type: application/javascript+module\r\n\r\nexport default {{}};\r\n").as_bytes());
        }
        output.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
        assert!(output.len() <= MAX_UPLOAD_BYTES);
        output
    }

    pub(crate) fn content_type() -> String {
        format!("multipart/form-data; boundary={BOUNDARY}")
    }
}
