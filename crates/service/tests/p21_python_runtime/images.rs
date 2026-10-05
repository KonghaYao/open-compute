//! Shared raster input and decoded output on the ordinary Images binding path.

use super::python_support::fixture::{Fixture, RequestTarget};
use super::{PEER, PYTHON};
use base64::Engine as _;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::{Value, json};
use std::io::Cursor;

pub(super) async fn parity(fixture: &Fixture) {
    let mut source = Vec::new();
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([10, 20, 30, 255])))
        .write_to(&mut Cursor::new(&mut source), ImageFormat::Png)
        .unwrap();
    let base64 = base64::engine::general_purpose::STANDARD;
    for (operation, format, stream, dimensions) in [
        ("transform", "image/png", false, (4, 3)),
        ("transform", "image/jpeg", false, (4, 3)),
        ("transform", "image/webp", false, (4, 3)),
        ("draw", "image/png", false, (3, 4)),
        ("transform", "image/png", true, (4, 3)),
    ] {
        let payload = json!({"source":base64.encode(&source),"operation":operation,"format":format,"image":stream});
        let mut javascript = None;
        for (script, caller) in [(PEER, "javascript"), (PYTHON, "sdk"), (PYTHON, "ffi")] {
            let result = call(fixture, script, caller, &payload).await;
            assert_eq!(
                result["info"],
                json!({"format":"png","width":2,"height":2,"fileSize":source.len()})
            );
            assert_eq!(result["contentType"], format);
            assert_eq!(result["status"], 200);
            if stream {
                assert!(result["header"].is_null());
                assert!(result["responseContentType"].is_null());
            } else {
                assert_eq!(result["header"], "custom");
                assert_eq!(result["responseContentType"], format);
            }
            let bytes = base64.decode(result["bytes"].as_str().unwrap()).unwrap();
            let actual_format = image::guess_format(&bytes).unwrap();
            let expected_format = match format {
                "image/png" => ImageFormat::Png,
                "image/jpeg" => ImageFormat::Jpeg,
                "image/webp" => ImageFormat::WebP,
                _ => unreachable!(),
            };
            assert_eq!(actual_format, expected_format);
            let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
            assert_eq!(decoded.dimensions(), dimensions);
            assert!(decoded.pixels().all(|pixel| pixel[3] == 255));
            if let Some(javascript) = &javascript {
                assert_eq!(
                    &result, javascript,
                    "Images differed for {caller}/{operation}"
                );
            } else {
                javascript = Some(result);
            }
        }
    }
    let payload = json!({"source":base64.encode(&source),"operation":"errors"});
    for (script, caller) in [(PEER, "javascript"), (PYTHON, "sdk"), (PYTHON, "ffi")] {
        let result = call(fixture, script, caller, &payload).await;
        for (operation, code) in [
            ("input", "IMAGE_INPUT_INVALID"),
            ("options", "IMAGE_OPTION_UNSUPPORTED"),
            ("decode", "IMAGE_INPUT_INVALID"),
        ] {
            let error = result["rejected"][operation].as_str().unwrap();
            assert!(error.contains(code), "unexpected Images error: {error}");
            for private in [
                "binding-backend",
                "x-open-compute",
                "descriptorSha256",
                "/internal/",
                "startup-generation",
            ] {
                assert!(
                    !error.contains(private),
                    "Images error exposed private authority"
                );
            }
        }
    }
}

async fn call(fixture: &Fixture, script: &str, caller: &str, payload: &Value) -> Value {
    let (status, _, body) = fixture
        .request(
            &format!("/images?caller={caller}"),
            "POST",
            "application/json",
            serde_json::to_vec(payload).unwrap(),
            RequestTarget::Worker(script),
        )
        .await;
    assert_eq!(status, 200, "Images invocation failed for {caller}");
    serde_json::from_slice(&body).unwrap()
}
