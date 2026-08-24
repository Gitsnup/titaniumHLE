/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! gzip compression functions
//!
//! This module provides gzip compression and decompression functions
//! using the flate2 crate.

use crate::libc;
use crate::mem::guest_size_t;
use crate::mem::MutGuestPtr;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{Read, Write};

pub struct State;

impl Default for State {
    fn default() -> Self {
        State
    }
}

fn gzip_compress(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)
        .map_err(|e| format!("Gzip encoding error: {}", e))?;
    encoder.finish()
        .map_err(|e| format!("Gzip finish error: {}", e))
}

fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = GzDecoder::new(data);
    let mut result = Vec::new();
    decoder.read_to_end(&mut result)
        .map_err(|e| format!("Gzip decoding error: {}", e))?;
    Ok(result)
}

// Host functions for gzip compression/decompression
pub const FUNCTIONS: &[(&str, usize)] = &[
    ("_gzip_compress", gzip_compress as usize),
    ("_gzip_decompress", gzip_decompress as usize),
    ("_compress", gzip_compress as usize),
    ("_decompress", gzip_decompress as usize),
];
