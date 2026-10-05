//! Genuine Python Durable Object deployment, shared RPC/storage and fresh-daemon recovery.

#![cfg(feature = "test-support")]

#[path = "../python_support/mod.rs"]
mod python_support;

mod assertions;
mod durable_objects;
mod websockets;

const PYTHON: &str = "python-durable-objects-fixture";
const PEER: &str = "python-durable-objects-javascript";
