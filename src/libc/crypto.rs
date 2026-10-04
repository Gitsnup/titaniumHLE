/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! CommonCrypto and friends

use crate::dyld::FunctionExports;
use crate::mem::{ConstVoidPtr, GuestUSize, MutPtr, MutVoidPtr};
use crate::{export_c_func, Environment};
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::{Aes128, Aes192, Aes256};
use digest::Digest;
use md5::Md5;
use sha1::Sha1;

fn CC_MD5(env: &mut Environment, data: ConstVoidPtr, len: u32, md: MutPtr<u8>) -> MutPtr<u8> {
    let mut hasher = Md5::new();
    hasher.update(env.mem.bytes_at(data.cast(), len));
    let digest = hasher.finalize();
    env.mem.bytes_at_mut(md, 16).copy_from_slice(&digest[..]);
    md
}

fn CC_SHA1(env: &mut Environment, data: ConstVoidPtr, len: u32, md: MutPtr<u8>) -> MutPtr<u8> {
    let mut hasher = Sha1::new();
    hasher.update(env.mem.bytes_at(data.cast(), len));
    let digest = hasher.finalize();
    env.mem.bytes_at_mut(md, 20).copy_from_slice(&digest[..]);
    md
}

/// AES block cipher operation, for `CCCrypt` (`kCCAlgorithmAES`).
enum AesMode {
    Aes128(Aes128),
    Aes192(Aes192),
    Aes256(Aes256),
}

impl AesMode {
    fn new(key: &[u8]) -> Option<Self> {
        match key.len() {
            16 => Some(AesMode::Aes128(Aes128::new(key.into()))),
            24 => Some(AesMode::Aes192(Aes192::new(key.into()))),
            32 => Some(AesMode::Aes256(Aes256::new(key.into()))),
            _ => None,
        }
    }

    fn encrypt_block(&self, block: &mut aes::Block) {
        match self {
            AesMode::Aes128(cipher) => cipher.encrypt_block(block),
            AesMode::Aes192(cipher) => cipher.encrypt_block(block),
            AesMode::Aes256(cipher) => cipher.encrypt_block(block),
        }
    }

    fn decrypt_block(&self, block: &mut aes::Block) {
        match self {
            AesMode::Aes128(cipher) => cipher.decrypt_block(block),
            AesMode::Aes192(cipher) => cipher.decrypt_block(block),
            AesMode::Aes256(cipher) => cipher.decrypt_block(block),
        }
    }
}

const K_CC_SUCCESS: i32 = 0;
const K_CC_PARAM_ERROR: i32 = -4300;
const K_CC_BUFFER_TOO_SMALL: i32 = -4301;
const K_CC_ALIGNMENT_ERROR: i32 = -4303;
const K_CC_DECODE_ERROR: i32 = -4304;
const K_CC_OP_DECRYPT: u32 = 1;
const K_CC_ALG_AES: u32 = 0;
const K_CC_OPT_PKCS7_PADDING: u32 = 0x1;
const K_CC_OPT_ECB_MODE: u32 = 0x2;
const K_CC_BLOCK_SIZE: u32 = 16;

/// `int CCCrypt(CCOperation op, CCAlgorithm alg, CCOptions options,
///              const void *key, size_t keyLength, const void *iv,
///              const void *dataIn, size_t dataInLength,
///              void *dataOut, size_t dataOutAvailable, size_t *dataOutMoved)`
///
/// Currently only supports AES in ECB or CBC mode, with or without PKCS#7
/// padding. Other algorithms return `kCCParamError`.
#[allow(clippy::too_many_arguments)]
fn CCCrypt(
    env: &mut Environment,
    op: u32,
    alg: u32,
    options: u32,
    key: ConstVoidPtr,
    key_length: GuestUSize,
    iv: ConstVoidPtr,
    data_in: ConstVoidPtr,
    data_in_length: GuestUSize,
    data_out: MutVoidPtr,
    data_out_available: GuestUSize,
    data_out_moved: MutPtr<GuestUSize>,
) -> i32 {
    let write_data_out_moved = |env: &mut Environment, value: GuestUSize| {
        if !data_out_moved.is_null() {
            env.mem.write(data_out_moved, value);
        }
    };
    if alg != K_CC_ALG_AES || (key_length != 16 && key_length != 24 && key_length != 32) {
        log!(
            "TODO: CCCrypt with unsupported algorithm ({}) or key size ({})",
            alg,
            key_length
        );
        write_data_out_moved(env, 0);
        return K_CC_PARAM_ERROR;
    }
    let ecb_mode = options & K_CC_OPT_ECB_MODE != 0;
    log!(
        "CCCrypt(op={}, alg={}, options={:#x}, key_len={}, iv={:#x}, in_len={})",
        op,
        alg,
        options,
        key_length,
        iv.to_bits(),
        data_in_length
    );
    // On iOS, a NULL IV in CBC mode means a zero-filled IV.
    let mut iv: Option<[u8; 16]> = if ecb_mode {
        None
    } else if iv.to_bits() != 0 {
        Some(
            env.mem
                .bytes_at(iv.cast(), K_CC_BLOCK_SIZE)
                .try_into()
                .unwrap(),
        )
    } else {
        Some([0; 16])
    };
    if !data_in_length.is_multiple_of(K_CC_BLOCK_SIZE) && (options & K_CC_OPT_PKCS7_PADDING) == 0 {
        log!("TODO: CCCrypt with unaligned input and no padding");
        write_data_out_moved(env, 0);
        return K_CC_ALIGNMENT_ERROR;
    }

    let key = env.mem.bytes_at(key.cast(), key_length).to_vec();
    let cipher = AesMode::new(&key).unwrap();
    let encrypting = op != K_CC_OP_DECRYPT;
    let mut data = if data_in_length == 0 {
        Vec::new()
    } else {
        env.mem.bytes_at(data_in.cast(), data_in_length).to_vec()
    };

    // Add PKCS#7 padding for encryption.
    if encrypting && options & K_CC_OPT_PKCS7_PADDING != 0 {
        let padding = 16 - (data.len() % 16);
        data.extend(std::iter::repeat_n(padding as u8, padding));
    }
    if !data.len().is_multiple_of(16) {
        // Can only happen if padding wasn't applied, i.e. decrypting.
        write_data_out_moved(env, 0);
        return K_CC_ALIGNMENT_ERROR;
    }

    // The output is never longer than the padded input.
    if data.len() > data_out_available as usize {
        write_data_out_moved(env, 0);
        return K_CC_BUFFER_TOO_SMALL;
    }
    for block in data.chunks_mut(K_CC_BLOCK_SIZE as usize) {
        let block: &mut [u8; 16] = block.try_into().unwrap();
        if encrypting {
            if let Some(prev) = &iv {
                for (byte, prev) in block.iter_mut().zip(prev) {
                    *byte ^= prev;
                }
            }
            cipher.encrypt_block(block.into());
            iv = Some(*block);
        } else {
            let cipher_block = *block;
            cipher.decrypt_block(block.into());
            if let Some(prev) = &iv {
                for (byte, prev) in block.iter_mut().zip(prev) {
                    *byte ^= prev;
                }
            }
            iv = Some(cipher_block);
        }
    }

    let mut out_len: u32 = data.len() as u32;
    if !encrypting && options & K_CC_OPT_PKCS7_PADDING != 0 {
        if data.is_empty() {
            log!("TODO: CCCrypt decrypt requested with empty padded input");
            write_data_out_moved(env, 0);
            return K_CC_DECODE_ERROR;
        }
        let padding = data[data.len() - 1];
        if padding == 0 || padding as u32 > K_CC_BLOCK_SIZE {
            log!("TODO: CCCrypt with invalid PKCS#7 padding");
            write_data_out_moved(env, 0);
            return K_CC_DECODE_ERROR;
        }
        out_len -= padding as u32;
    }
    if out_len != 0 && data_out.is_null() {
        write_data_out_moved(env, 0);
        return K_CC_PARAM_ERROR;
    }
    if out_len != 0 {
        env.mem
            .bytes_at_mut(data_out.cast(), out_len)
            .copy_from_slice(&data[..out_len as usize]);
    }
    write_data_out_moved(env, out_len);
    K_CC_SUCCESS
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CC_MD5(_, _, _)),
    export_c_func!(CC_SHA1(_, _, _)),
    export_c_func!(CCCrypt(_, _, _, _, _, _, _, _, _, _, _)),
];

#[cfg(test)]
mod tests {
    use super::*;

    // FIPS-197 Appendix C.1: AES-128 test vector.
    #[test]
    fn aes128_fips197_vector() {
        let key = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let plaintext = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        let expected_ciphertext = [
            0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4,
            0xc5, 0x5a,
        ];
        let cipher = AesMode::new(&key).unwrap();
        let mut block = plaintext;
        cipher.encrypt_block((&mut block).into());
        assert_eq!(block, expected_ciphertext);
        cipher.decrypt_block((&mut block).into());
        assert_eq!(block, plaintext);
    }

    // NIST SP 800-38A F.2.5: CBC-AES256, two blocks.
    #[test]
    fn aes256_cbc_nist_vector() {
        let key: [u8; 32] = [
            0x60, 0x3d, 0xeb, 0x10, 0x15, 0xca, 0x71, 0xbe, 0x2b, 0x73, 0xae, 0xf0, 0x85, 0x7d,
            0x77, 0x81, 0x1f, 0x35, 0x2c, 0x07, 0x3b, 0x61, 0x08, 0xd7, 0x2d, 0x98, 0x10, 0xa3,
            0x09, 0x14, 0xdf, 0xf4,
        ];
        let iv: [u8; 16] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let plaintext: [u8; 32] = [
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93,
            0x17, 0x2a, 0xae, 0x2d, 0x8a, 0x57, 0x1e, 0x03, 0xac, 0x9c, 0x9e, 0xb7, 0x6f, 0xac,
            0x45, 0xaf, 0x8e, 0x51,
        ];
        let expected: [u8; 32] = [
            0xf5, 0x8c, 0x4c, 0x04, 0xd6, 0xe5, 0xf1, 0xba, 0x77, 0x9e, 0xab, 0xfb, 0x5f, 0x7b,
            0xfb, 0xd6, 0x9c, 0xfc, 0x4e, 0x96, 0x7e, 0xdb, 0x80, 0x8d, 0x67, 0x9f, 0x77, 0x7b,
            0xc6, 0x70, 0x2c, 0x7d,
        ];
        let cipher = AesMode::new(&key).unwrap();

        let mut data = plaintext;
        let mut prev = iv;
        for block in data.chunks_mut(16) {
            let block: &mut [u8; 16] = block.try_into().unwrap();
            for (byte, p) in block.iter_mut().zip(&prev) {
                *byte ^= p;
            }
            cipher.encrypt_block(block.into());
            prev = *block;
        }
        assert_eq!(data, expected);

        let mut prev = iv;
        for block in data.chunks_mut(16) {
            let block: &mut [u8; 16] = block.try_into().unwrap();
            let cipher_block = *block;
            cipher.decrypt_block(block.into());
            for (byte, p) in block.iter_mut().zip(&prev) {
                *byte ^= p;
            }
            prev = cipher_block;
        }
        assert_eq!(data, plaintext);
    }
}
