//! Genuine Python Workflow uploads and durable ordinary-daemon scheduler recovery.

#![cfg(feature = "test-support")]

#[path = "../python_support/mod.rs"]
mod python_support;

mod assertions;
mod workflows;

const PYTHON: &str = "python-workflows-fixture";
const PEER: &str = "python-workflows-javascript";
const FLOW: &str = "python-workflows-flow";
const PEER_FLOW: &str = "python-workflows-javascript-flow";
