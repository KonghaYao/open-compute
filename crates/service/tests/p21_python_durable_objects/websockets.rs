//! Public HTTP upgrades, native Python/JS events and persisted socket counters.

use super::python_support::fixture::{Fixture, RequestTarget};
use super::python_support::{PYTHON_SECRETS, READ_ONLY_TOKEN, TOKEN};
use super::{PEER, PYTHON};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hyper_util::rt::TokioIo;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

type Socket = TokioIo<hyper::upgrade::Upgraded>;

pub(super) async fn verify(fixture: &Fixture, round: u64) {
    for caller in [PYTHON, PEER] {
        let path = "/socket";
        let (status, _, _) = fixture
            .request(
                path,
                "GET",
                "application/json",
                Vec::new(),
                RequestTarget::Worker(caller),
            )
            .await;
        assert_eq!(status, 426, "WebSocket route must require Upgrade");
        let request = Request::builder()
            .uri(format!("http://{}{path}", fixture.public))
            .header(
                header::HOST,
                format!("{caller}.{}.localhost", fixture.internal_account),
            )
            .header(header::CONNECTION, "Upgrade")
            .header(header::UPGRADE, "websocket")
            .header(header::SEC_WEBSOCKET_VERSION, "13")
            .header(header::SEC_WEBSOCKET_KEY, "AAECAwQFBgcICQoLDA0ODw==")
            .body(Body::empty())
            .unwrap();
        let mut response =
            tokio::time::timeout(Duration::from_secs(30), fixture.client.request(request))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert_eq!(
            response.headers()[header::SEC_WEBSOCKET_ACCEPT],
            "Bz3qJYTGdOe8gUSpLosEdiLKDrk="
        );
        for secret in PYTHON_SECRETS.into_iter().chain([TOKEN, READ_ONLY_TOKEN]) {
            assert!(response.headers().values().all(|value| {
                !value
                    .as_bytes()
                    .windows(secret.len())
                    .any(|part| part == secret.as_bytes())
            }));
        }
        let upgrade =
            tokio::time::timeout(Duration::from_secs(5), hyper::upgrade::on(&mut response))
                .await
                .unwrap()
                .unwrap();
        let mut socket = TokioIo::new(upgrade);
        for (opcode, payload) in [(0x81, "hello µ☁".as_bytes()), (0x82, &[0, 255, 1, 128][..])] {
            write_frame(&mut socket, opcode, payload).await;
            assert_eq!(read_frame(&mut socket, opcode).await, payload);
        }
        write_frame(&mut socket, 0x88, &[3, 232]).await;
        assert_eq!(read_frame(&mut socket, 0x88).await, [3, 232]);
        let mut tail = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut tail))
            .await
            .unwrap()
            .unwrap();
        assert!(tail.is_empty());
    }
    counters(fixture, round).await;
}

pub(super) async fn counters(fixture: &Fixture, round: u64) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let python = fixture.invoke(PYTHON, "/read").await;
        let local = fixture.invoke(PEER, "/read").await;
        if python["socketCloses"] == round && local["socketCloses"] == round {
            assert_eq!(python["socketMessages"], round * 2);
            assert_eq!(local["socketMessages"], round * 2);
            assert_eq!(python["socketCloseClean"], true);
            assert_eq!(local["socketCloseClean"], true);
            return;
        }
        assert!(
            Instant::now() < deadline,
            "native close events were not committed"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn write_frame(socket: &mut Socket, opcode: u8, payload: &[u8]) {
    assert!(payload.len() < 126);
    let mask = [1, 2, 3, 4];
    let mut bytes = vec![opcode, 0x80 | u8::try_from(payload.len()).unwrap()];
    bytes.extend_from_slice(&mask);
    bytes.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    tokio::time::timeout(Duration::from_secs(5), socket.write_all(&bytes))
        .await
        .unwrap()
        .unwrap();
}

async fn read_frame(socket: &mut Socket, opcode: u8) -> Vec<u8> {
    let mut header = [0; 2];
    tokio::time::timeout(Duration::from_secs(5), socket.read_exact(&mut header))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(header[0], opcode, "one complete native frame expected");
    assert!(header[1] < 126, "unmasked bounded server frame expected");
    let mut bytes = vec![0; usize::from(header[1])];
    tokio::time::timeout(Duration::from_secs(5), socket.read_exact(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    bytes
}
