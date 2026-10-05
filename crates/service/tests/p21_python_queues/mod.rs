//! Official Python Queue producer/consumer input against real durable daemon authority.

#![cfg(feature = "test-support")]

#[path = "../python_support/mod.rs"]
mod python_support;

mod assertions;
mod queues;

const PYTHON: &str = "python-queues-fixture";
const PEER: &str = "python-queues-javascript";
const EVENTS: &str = "python-queues-events";
const DLQ: &str = "python-queues-dlq";
