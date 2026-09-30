/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSUbiquitousKeyValueStore`.
//!
//! This is iCloud key-value storage: `NSUserDefaults` mirrored to the cloud.
//! There is no iCloud here, so this is an in-process key-value store: writes
//! round-trip within the run and `synchronize` always succeeds. Values do not
//! survive an app relaunch; apps are expected to keep their own local save
//! and treat this store as a mirror.

use super::ns_string::to_rust_string;
use crate::dyld::{ConstantExports, HostConstant};
use crate::objc::{id, msg, nil, objc_classes, retain, ClassExports, HostObject, NSZonePtr};
use std::collections::HashMap;

pub struct NSUbiquitousKeyValueStoreHostObject {
    /// Backing store, keyed by the key's string contents. Retained.
    values: HashMap<String, id>,
}
impl HostObject for NSUbiquitousKeyValueStoreHostObject {}

#[derive(Default)]
pub struct State {
    /// `NSUbiquitousKeyValueStore*`
    store: Option<id>,
}

pub const NSUbiquitousKeyValueStoreDidChangeExternallyNotification: &str =
    "NSUbiquitousKeyValueStoreDidChangeExternallyNotification";
pub const NSUbiquitousKeyValueStoreChangeReasonKey: &str =
    "NSUbiquitousKeyValueStoreChangeReasonKey";

pub const CONSTANTS: ConstantExports = &[
    (
        "_NSUbiquitousKeyValueStoreDidChangeExternallyNotification",
        HostConstant::NSString(NSUbiquitousKeyValueStoreDidChangeExternallyNotification),
    ),
    (
        "_NSUbiquitousKeyValueStoreChangeReasonKey",
        HostConstant::NSString(NSUbiquitousKeyValueStoreChangeReasonKey),
    ),
];

pub const CLASSES: ClassExports = objc_classes! {
(env, this, _cmd);

@implementation NSUbiquitousKeyValueStore: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSUbiquitousKeyValueStoreHostObject {
        values: HashMap::new(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)defaultStore {
    if let Some(existing) = env.framework_state.foundation.ns_ubiquitous_key_value_store.store {
        existing
    } else {
        let store: id = msg![env; this new];
        env.framework_state.foundation.ns_ubiquitous_key_value_store.store = Some(store);
        store
    }
}

- (bool)setObject:(id)object
           forKey:(id)key { // (NSObject*, NSString*)
    let key = to_rust_string(env, key).into_owned();
    if object == nil {
        // Documented: setting nil removes the key.
        env.objc
            .borrow_mut::<NSUbiquitousKeyValueStoreHostObject>(this)
            .values
            .remove(&key);
    } else {
        // Retain before borrowing the map: `retain` needs `env` mutably.
        retain(env, object);
        env.objc
            .borrow_mut::<NSUbiquitousKeyValueStoreHostObject>(this)
            .values
            .insert(key, object);
    }
    true
}

- (id)objectForKey:(id)key { // (NSString*) -> NSObject*
    let key = to_rust_string(env, key);
    let value = *env
        .objc
        .borrow::<NSUbiquitousKeyValueStoreHostObject>(this)
        .values
        .get(key.as_ref())
        .unwrap_or(&nil);
    if value != nil {
        retain(env, value);
    }
    value
}

- (())removeObjectForKey:(id)key { // (NSString*)
    let key = to_rust_string(env, key);
    env.objc
        .borrow_mut::<NSUbiquitousKeyValueStoreHostObject>(this)
        .values
        .remove(key.as_ref());
}

- (bool)synchronize {
    // Nothing to flush anywhere.
    true
}

- (bool)boolForKey:(id)_key { // (NSString*) -> BOOL
    // All values default to NO.
    false
}

- (i64)longLongForKey:(id)_key { // (NSString*) -> long long
    0
}

- (f64)doubleForKey:(id)_key { // (NSString*) -> double
    0.0
}

@end

};
