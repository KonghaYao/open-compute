//! Python Main Gate against the ordinary daemon, formal runtime and reviewed cf upload.

#![cfg(feature = "test-support")]

#[path = "../python_support/mod.rs"]
mod python_support;

mod authorization;
mod bindings;
mod python_main;

use python_support::{PYTHON_SECRETS, capture, fixture, platform_process};

const SCRIPT: &str = "python-main-fixture";
