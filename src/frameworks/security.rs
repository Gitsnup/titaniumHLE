/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */
//! Security framework (minimal, keychain-only).
//!
//! The keychain is not actually implemented; queries simply behave as if the
//! keychain were empty. This is enough for apps that use the keychain
//! opportunistically (e.g. to store account credentials) without crashing.
//!
//! Resources:
//! - Apple's [Keychain Services Programming Guide](https://developer.apple.com/library/archive/documentation/Security/Conceptual/keychainServConcepts/02concepts/concepts.html)

use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::core_foundation::{cf_dictionary::CFDictionaryRef, CFTypeRef};
use crate::mem::GuestUSize;
use crate::mem::{MutPtr, MutVoidPtr};
use crate::Environment;

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/Security.framework/Security",
    aliases: &[],
    class_exports: &[],
    constant_exports: &[CONSTANTS],
    function_exports: &[FUNCTIONS],
};

/// The keychain query keys are `CFStringRef` constants. Their string values
/// are irrelevant for emulation as long as they are consistent.
pub const CONSTANTS: ConstantExports = &[
    ("_kSecClass", HostConstant::NSString("kSecClass")),
    (
        "_kSecClassGenericPassword",
        HostConstant::NSString("kSecClassGenericPassword"),
    ),
    (
        "_kSecAttrAccount",
        HostConstant::NSString("kSecAttrAccount"),
    ),
    (
        "_kSecAttrService",
        HostConstant::NSString("kSecAttrService"),
    ),
    ("_kSecAttrLabel", HostConstant::NSString("kSecAttrLabel")),
    ("_kSecReturnData", HostConstant::NSString("kSecReturnData")),
    (
        "_kSecReturnAttributes",
        HostConstant::NSString("kSecReturnAttributes"),
    ),
    // Additional keychain constants referenced by some apps (e.g. Where's My
    // Water). Their values are irrelevant as long as they are consistent.
    ("_kSecValueData", HostConstant::NSString("kSecValueData")),
    ("_kSecAttrServer", HostConstant::NSString("kSecAttrServer")),
    ("_kSecAttrType", HostConstant::NSString("kSecAttrType")),
    (
        "_kSecAttrSecurityDomain",
        HostConstant::NSString("kSecAttrSecurityDomain"),
    ),
    (
        "_kSecAttrAuthenticationType",
        HostConstant::NSString("kSecAttrAuthenticationType"),
    ),
    (
        "_kSecAttrAuthenticationTypeDefault",
        HostConstant::NSString("kSecAttrAuthenticationTypeDefault"),
    ),
    (
        "_kSecClassInternetPassword",
        HostConstant::NSString("kSecClassInternetPassword"),
    ),
    ("_kSecMatchLimit", HostConstant::NSString("kSecMatchLimit")),
    (
        "_kSecMatchLimitOne",
        HostConstant::NSString("kSecMatchLimitOne"),
    ),
    (
        "_kSecReturnPersistentRef",
        HostConstant::NSString("kSecReturnPersistentRef"),
    ),
];

// OSStatus values from SecBase.h
pub const ERR_SEC_SUCCESS: i32 = 0;
// errSecItemNotFound as exposed by the guest ABI
const ERR_SEC_ITEM_NOT_FOUND_GUEST: u32 = (-25300i32) as u32;

/// OSStatus
fn SecItemCopyMatching(
    _env: &mut Environment,
    _query: CFDictionaryRef,
    _result: MutPtr<CFTypeRef>,
) -> u32 {
    log!(
        "TODO: SecItemCopyMatching({:?}, {:?}): keychain is not implemented, returning errSecItemNotFound",
        _query,
        _result
    );
    // TODO: write to result on success
    ERR_SEC_ITEM_NOT_FOUND_GUEST
}

/// OSStatus
fn SecItemAdd(
    _env: &mut Environment,
    _attributes: CFDictionaryRef,
    _result: MutPtr<CFTypeRef>,
) -> u32 {
    log!(
        "TODO: SecItemAdd({:?}, {:?}): keychain is not implemented, returning errSecSuccess",
        _attributes,
        _result
    );
    ERR_SEC_SUCCESS as u32
}

/// OSStatus
fn SecItemUpdate(
    _env: &mut Environment,
    _query: CFDictionaryRef,
    _attributes_to_update: CFDictionaryRef,
) -> u32 {
    log!(
        "TODO: SecItemUpdate({:?}, {:?}): keychain is not implemented, returning errSecSuccess",
        _query,
        _attributes_to_update
    );
    ERR_SEC_SUCCESS as u32
}

/// OSStatus
fn SecItemDelete(_env: &mut Environment, _query: CFDictionaryRef) -> u32 {
    log!(
        "TODO: SecItemDelete({:?}): keychain is not implemented, returning errSecSuccess",
        _query
    );
    ERR_SEC_SUCCESS as u32
}

/// OSStatus
fn SecRandomCopyBytes(
    _env: &mut Environment,
    _rnd: i32,
    count: GuestUSize,
    buffer: MutVoidPtr,
) -> u32 {
    log!(
        "TODO: SecRandomCopyBytes({}, {}, {:?}): using pseudo-random bytes",
        _rnd,
        count,
        buffer
    );
    let mut byte = 0x5au8;
    for i in 0..count {
        byte = byte
            .wrapping_mul(1103515245u32 as u8)
            .wrapping_add(12345u32 as u8);
        _env.mem.write(buffer.cast::<u8>() + i, byte);
    }
    ERR_SEC_SUCCESS as u32
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(SecItemCopyMatching(_, _)),
    export_c_func!(SecItemAdd(_, _)),
    export_c_func!(SecItemUpdate(_, _)),
    export_c_func!(SecItemDelete(_)),
    export_c_func!(SecRandomCopyBytes(_, _, _)),
];
