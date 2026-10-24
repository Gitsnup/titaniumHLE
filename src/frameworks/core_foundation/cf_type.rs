/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CFType` (type-generic functions etc).

use super::{CFHashCode, CFIndex};
use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::foundation::NSUInteger;
use crate::objc::Class;
use crate::{msg, objc};
use crate::{msg_class, Environment};

pub type CFTypeRef = objc::id;

pub fn CFRetain(env: &mut Environment, object: CFTypeRef) -> CFTypeRef {
    assert!(!object.is_null()); // not allowed, unlike for normal objc objects
    objc::retain(env, object)
}
pub fn CFRelease(env: &mut Environment, object: CFTypeRef) {
    objc::release(env, object);
}

/// A Core Foundation type identifier, as returned by `CFGetTypeID` and
/// friends. Type IDs are process-stable; we assign fixed IDs rather than
/// runtime-registered ones, which is compatible with guests that only
/// compare against values returned by the other `CF*GetTypeID` functions.
pub type CFTypeID = u32;

pub const CF_STRING_TYPE_ID: CFTypeID = 1;
pub const CF_NUMBER_TYPE_ID: CFTypeID = 2;
pub const CF_BOOLEAN_TYPE_ID: CFTypeID = 3;
pub const CF_DATA_TYPE_ID: CFTypeID = 4;
pub const CF_ARRAY_TYPE_ID: CFTypeID = 5;
pub const CF_DICTIONARY_TYPE_ID: CFTypeID = 6;
pub const CF_URL_TYPE_ID: CFTypeID = 7;
/// Returned for objects of unrecognised classes; never 0, which Apple
/// defines as an invalid type ID.
pub const CF_UNKNOWN_TYPE_ID: CFTypeID = 0xFF;

/// Legacy garbage-collection-era function. With the GC long gone this is
/// equivalent to `CFRetain` (which is how modern CoreFoundation implements
/// it), but guests only ever used it to satisfy an API, so returning the
/// object itself is also fine.
pub fn CFMakeCollectable(env: &mut Environment, object: CFTypeRef) -> CFTypeRef {
    CFRetain(env, object)
}

pub fn CFGetRetainCount(env: &mut Environment, object: CFTypeRef) -> CFIndex {
    let count: NSUInteger = msg![env; object retainCount];
    count as CFIndex
}

pub fn CFEqual(env: &mut Environment, object1: CFTypeRef, object2: CFTypeRef) -> bool {
    if object1 == object2 {
        return true;
    }
    // TODO: other classes
    let str_class: Class = msg_class![env; NSString class];
    let object1_class: Class = msg![env; object1 class];
    assert!(msg![env; object1_class isKindOfClass:str_class]);
    let object2_class: Class = msg![env; object2 class];
    assert!(msg![env; object2_class isKindOfClass:str_class]);
    // TODO: use isEqual: once it is fixed
    msg![env; object1 isEqualToString:object2]
}

pub fn CFHash(env: &mut Environment, object: CFTypeRef) -> CFHashCode {
    msg![env; object hash]
}
pub fn CFGetTypeID(env: &mut Environment, cf_type: CFTypeRef) -> CFTypeID {
    let class: Class = msg![env; cf_type class];
    let mut is_kind_of = |name: &str| -> bool {
        let known_class = env.objc.get_known_class(name, &mut env.mem);
        env.objc.class_is_subclass_of(class, known_class)
    };
    if is_kind_of("NSString") {
        CF_STRING_TYPE_ID
    } else if is_kind_of("NSNumber") {
        CF_NUMBER_TYPE_ID
    } else if is_kind_of("NSData") {
        CF_DATA_TYPE_ID
    } else if is_kind_of("NSArray") {
        CF_ARRAY_TYPE_ID
    } else if is_kind_of("NSDictionary") {
        CF_DICTIONARY_TYPE_ID
    } else if is_kind_of("NSURL") {
        CF_URL_TYPE_ID
    } else {
        log_dbg!(
            "CFGetTypeID: unrecognised class {}",
            env.objc.get_class_name(class)
        );
        CF_UNKNOWN_TYPE_ID
    }
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFRetain(_)),
    export_c_func!(CFRelease(_)),
    export_c_func!(CFMakeCollectable(_)),
    export_c_func!(CFGetRetainCount(_)),
    export_c_func!(CFEqual(_, _)),
    export_c_func!(CFHash(_)),
    export_c_func!(CFGetTypeID(_)),
];
