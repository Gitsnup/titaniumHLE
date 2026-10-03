/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! A minimal PNG encoder, used by `UIImagePNGRepresentation`.

use super::Image;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn write_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(chunk_type);
    hasher.update(data);
    let crc = hasher.finalize();

    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encodes an [Image] as an 8-bit RGBA PNG. The image's premultiplied-alpha
/// sRGB pixels are un-premultiplied first, as that's what PNG expects.
pub fn encode_png(image: &Image) -> Vec<u8> {
    let (width, height) = image.dimensions();

    let mut raw: Vec<u8> = Vec::with_capacity((width * height * 4 + height) as usize);
    for row in 0..height {
        raw.push(0); // filter type: None
        for col in 0..width {
            let idx = ((row * width + col) * 4) as usize;
            let px = &image.pixels()[idx..idx + 4];
            let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
            if a == 0 {
                raw.extend_from_slice(&[0, 0, 0, 0]);
            } else if a == 255 {
                raw.extend_from_slice(&[r, g, b, a]);
            } else {
                let un_premultiply =
                    |c: u8| -> u8 { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
                raw.extend_from_slice(&[
                    un_premultiply(r),
                    un_premultiply(g),
                    un_premultiply(b),
                    a,
                ]);
            }
        }
    }

    let mut compressed = ZlibEncoder::new(Vec::new(), Compression::default());
    compressed.write_all(&raw).unwrap();
    let compressed = compressed.finish().unwrap();

    let mut png = Vec::new();
    png.extend_from_slice(&PNG_SIGNATURE);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[
        8, // bit depth
        6, // color type: RGBA
        0, // compression: deflate
        0, // filter: adaptive (only None is used)
        0, // interlace: none
    ]);
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &compressed);
    write_chunk(&mut png, b"IEND", &[]);
    png
}
