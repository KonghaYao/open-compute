//! Ordinary Python SDK/raw FFI, standard-library and host-network qualification.

#![cfg(feature = "test-support")]

#[path = "../python_support/mod.rs"]
mod python_support;

mod ai_search;
mod ai_search_provider;
mod artifacts;
mod assertions;
mod assets;
mod cache;
mod dynamic;
mod http_clients;
mod images;
mod runtime;
mod search;

const PYTHON: &str = "python-runtime-fixture";
const PEER: &str = "python-runtime-javascript";
