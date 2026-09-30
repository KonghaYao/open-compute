use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

pub(super) struct PostgresFixture {
    pub(super) address: std::net::SocketAddr,
    events: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl PostgresFixture {
    pub(super) async fn finish(self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.task)
            .await
            .expect("PostgreSQL driver did not close its connection")
            .unwrap();
        assert_eq!(
            *self.events.lock().unwrap(),
            [
                "BEGIN",
                "SELECT:41",
                "COMMIT",
                "BEGIN",
                "SELECT:42",
                "ROLLBACK",
                "TERMINATE",
            ]
        );
    }
}

pub(super) async fn spawn() -> PostgresFixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let task_events = events.clone();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        serve(&mut stream, &task_events).await.unwrap();
    });
    PostgresFixture {
        address,
        events,
        task,
    }
}

async fn serve(
    stream: &mut tokio::net::TcpStream,
    events: &Arc<Mutex<Vec<String>>>,
) -> std::io::Result<()> {
    let startup = read_startup(stream).await?;
    assert_eq!(
        u32::from_be_bytes(startup[..4].try_into().unwrap()),
        196_608
    );
    write_message(stream, b'R', &0_u32.to_be_bytes()).await?;
    write_message(stream, b'S', b"server_version\x0016.0\0").await?;
    write_message(stream, b'S', b"client_encoding\0UTF8\0").await?;
    write_message(stream, b'K', &[0; 8]).await?;
    write_message(stream, b'Z', b"I").await?;

    let mut statements = BTreeMap::new();
    let mut portals = BTreeMap::new();
    let mut transaction = false;
    loop {
        let mut kind = [0_u8; 1];
        if stream.read_exact(&mut kind).await.is_err() {
            break;
        }
        let payload = read_payload(stream).await?;
        match kind[0] {
            b'P' => {
                let (name, offset) = cstring(&payload, 0);
                let (query, _) = cstring(&payload, offset);
                statements.insert(name.to_owned(), query.to_owned());
                write_message(stream, b'1', &[]).await?;
            }
            b'B' => {
                let (portal, offset) = cstring(&payload, 0);
                let (statement, mut offset) = cstring(&payload, offset);
                let format_count = read_i16(&payload, &mut offset) as usize;
                offset += format_count * 2;
                let parameter_count = read_i16(&payload, &mut offset) as usize;
                let mut parameters = Vec::with_capacity(parameter_count);
                for _ in 0..parameter_count {
                    let length = read_i32(&payload, &mut offset);
                    if length < 0 {
                        parameters.push(String::new());
                    } else {
                        let length = length as usize;
                        parameters.push(
                            String::from_utf8(payload[offset..offset + length].to_vec()).unwrap(),
                        );
                        offset += length;
                    }
                }
                let query = statements.get(statement).unwrap().clone();
                portals.insert(portal.to_owned(), (query, parameters));
                write_message(stream, b'2', &[]).await?;
            }
            b'D' => {
                let (name, _) = cstring(&payload, 1);
                let query = if payload[0] == b'P' {
                    &portals.get(name).unwrap().0
                } else {
                    statements.get(name).unwrap()
                };
                if query.starts_with("SELECT") {
                    write_row_description(stream).await?;
                } else {
                    write_message(stream, b'n', &[]).await?;
                }
            }
            b'E' => {
                let (portal, _) = cstring(&payload, 0);
                let (query, parameters) = portals.get(portal).unwrap();
                assert!(query.starts_with("SELECT $1::int AS value"));
                let value = parameters.first().unwrap();
                events.lock().unwrap().push(format!("SELECT:{value}"));
                let mut row = Vec::new();
                row.extend_from_slice(&1_i16.to_be_bytes());
                row.extend_from_slice(&(value.len() as i32).to_be_bytes());
                row.extend_from_slice(value.as_bytes());
                write_message(stream, b'D', &row).await?;
                write_message(stream, b'C', b"SELECT 1\0").await?;
            }
            b'Q' => {
                let query = std::str::from_utf8(&payload[..payload.len() - 1]).unwrap();
                events.lock().unwrap().push(query.to_owned());
                transaction = match query {
                    "BEGIN" => true,
                    "COMMIT" | "ROLLBACK" => false,
                    _ => panic!("unexpected PostgreSQL query"),
                };
                let tag = format!("{query}\0");
                write_message(stream, b'C', tag.as_bytes()).await?;
                write_message(stream, b'Z', if transaction { b"T" } else { b"I" }).await?;
            }
            b'S' => {
                write_message(stream, b'Z', if transaction { b"T" } else { b"I" }).await?;
            }
            b'C' => write_message(stream, b'3', &[]).await?,
            b'H' => stream.flush().await?,
            b'X' => {
                events.lock().unwrap().push("TERMINATE".to_owned());
                break;
            }
            other => panic!("unexpected PostgreSQL frontend message {other}"),
        }
    }
    Ok(())
}

async fn read_startup(stream: &mut tokio::net::TcpStream) -> std::io::Result<Vec<u8>> {
    let mut length = [0_u8; 4];
    stream.read_exact(&mut length).await?;
    let mut payload = vec![0; u32::from_be_bytes(length) as usize - 4];
    stream.read_exact(&mut payload).await?;
    Ok(payload)
}

async fn read_payload(stream: &mut tokio::net::TcpStream) -> std::io::Result<Vec<u8>> {
    let mut length = [0_u8; 4];
    stream.read_exact(&mut length).await?;
    let mut payload = vec![0; u32::from_be_bytes(length) as usize - 4];
    stream.read_exact(&mut payload).await?;
    Ok(payload)
}

async fn write_message(
    stream: &mut tokio::net::TcpStream,
    kind: u8,
    payload: &[u8],
) -> std::io::Result<()> {
    stream.write_u8(kind).await?;
    stream.write_u32((payload.len() + 4) as u32).await?;
    stream.write_all(payload).await?;
    stream.flush().await
}

async fn write_row_description(stream: &mut tokio::net::TcpStream) -> std::io::Result<()> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(b"value\0");
    payload.extend_from_slice(&0_u32.to_be_bytes());
    payload.extend_from_slice(&0_i16.to_be_bytes());
    payload.extend_from_slice(&23_u32.to_be_bytes());
    payload.extend_from_slice(&4_i16.to_be_bytes());
    payload.extend_from_slice(&(-1_i32).to_be_bytes());
    payload.extend_from_slice(&0_i16.to_be_bytes());
    write_message(stream, b'T', &payload).await
}

fn cstring(bytes: &[u8], offset: usize) -> (&str, usize) {
    let end = bytes[offset..].iter().position(|byte| *byte == 0).unwrap() + offset;
    (std::str::from_utf8(&bytes[offset..end]).unwrap(), end + 1)
}

fn read_i16(bytes: &[u8], offset: &mut usize) -> i16 {
    let value = i16::from_be_bytes(bytes[*offset..*offset + 2].try_into().unwrap());
    *offset += 2;
    value
}

fn read_i32(bytes: &[u8], offset: &mut usize) -> i32 {
    let value = i32::from_be_bytes(bytes[*offset..*offset + 4].try_into().unwrap());
    *offset += 4;
    value
}
