/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CFArray` and `CFMutableArray`.
//!
//! These are toll-free bridged to `NSArray` and `NSMutableArray` in Apple's
//! implementation. Here they are the same types.

use super::cf_allocator::{kCFAllocatorDefault, CFAllocatorRef};
use super::cf_dictionary::create_default_callback_functions;
use super::CFIndex;
use crate::abi::GuestFunction;
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::foundation::NSUInteger;
use crate::mem::{ConstVoidPtr, SafeRead};
use crate::objc::{id, msg, msg_class};
use crate::Environment;

#[allow(dead_code)]
pub type CFArrayRef = super::CFTypeRef;
pub type CFMutableArrayRef = super::CFTypeRef;

fn CFArrayCreateMutable(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    capacity: CFIndex,
    callbacks: ConstVoidPtr, // TODO, should be `const CFArrayCallBacks*`
) -> CFMutableArrayRef {
    assert!(allocator == kCFAllocatorDefault || env.mem.read(allocator).is_system_default()); // unimplemented
    assert!(capacity == 0); // TODO: fixed capacity support
    if !callbacks.is_null() {
        // TODO: the callbacks are effectively assumed to be
        // `kCFTypeArrayCallBacks` (i.e. retaining, like `NSMutableArray`); any
        // other callback structs are treated the same way.
        let version: CFIndex = env.mem.read(callbacks.cast());
        assert_eq!(version, 0);
        return msg_class![env; NSMutableArray new];
    }

    msg_class![env; _touchHLE_NSMutableArray_non_retaining new]
}

fn CFArrayGetCount(env: &mut Environment, array: CFArrayRef) -> CFIndex {
    let count: NSUInteger = msg![env; array count];
    count.try_into().unwrap()
}

fn CFArrayGetValueAtIndex(env: &mut Environment, array: CFArrayRef, idx: CFIndex) -> ConstVoidPtr {
    let idx: NSUInteger = idx.try_into().unwrap();
    let value: id = msg![env; array objectAtIndex:idx];
    value.cast().cast_const()
}

fn CFArrayAppendValue(env: &mut Environment, array: CFMutableArrayRef, value: ConstVoidPtr) {
    let value: id = value.cast().cast_mut();
    msg![env; array addObject:value]
}

fn CFArrayRemoveValueAtIndex(env: &mut Environment, array: CFMutableArrayRef, idx: CFIndex) {
    let idx: NSUInteger = idx.try_into().unwrap();
    msg![env; array removeObjectAtIndex:idx]
}

#[repr(C, packed)]
pub struct CFArrayCallBacks {
    pub version: CFIndex,         // version
    pub retain: GuestFunction,    // const void *(*retain)(CFAllocatorRef, const void *value)
    pub release: GuestFunction,   // void (*release)(CFAllocatorRef, const void *value)
    pub copy_desc: GuestFunction, // CFStringRef (*copyDescription)(const void *value)
    pub equal: GuestFunction,     // Boolean (*equal)(const void *value1, const void *value2)
}
unsafe impl SafeRead for CFArrayCallBacks {}

pub const CONSTANTS: ConstantExports = &[(
    "_kCFTypeArrayCallBacks",
    HostConstant::Custom(|env| {
        let common = create_default_callback_functions(&mut env.mem, &mut env.dyld);
        let callbacks = CFArrayCallBacks {
            version: 0, // always 0
            retain: common.retain,
            release: common.release,
            copy_desc: common.copy_desc,
            equal: common.equal,
        };
        env.mem.alloc_and_write(callbacks).cast_void().cast_const()
    }),
)];

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFArrayCreateMutable(_, _, _)),
    export_c_func!(CFArrayGetCount(_)),
    export_c_func!(CFArrayGetValueAtIndex(_, _)),
    export_c_func!(CFArrayAppendValue(_, _)),
    export_c_func!(CFArrayRemoveValueAtIndex(_, _)),
];
