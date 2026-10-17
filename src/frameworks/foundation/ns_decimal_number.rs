/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSDecimalNumber`.
//!
//! These are backed by a plain double, rather than an arbitrary-precision
//! decimal type. This is not fully faithful, but it is precise enough for
//! the games that use this class.

use super::ns_string;
use super::ns_value::NSNumberHostObject;
use crate::objc::{autorelease, id, msg, objc_classes, ClassExports, NSZonePtr};

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSDecimalNumber: NSNumber

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSNumberHostObject::Double(0.0));
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)decimalNumberWithString:(id)string { // NSString *
    let parsed: f64 = ns_string::to_rust_string(env, string).trim().parse().unwrap_or(0.0);
    let new: id = msg![env; this alloc];
    *env.objc.borrow_mut::<NSNumberHostObject>(new) = NSNumberHostObject::Double(parsed);
    autorelease(env, new)
}

- (id)initWithString:(id)string { // NSString *
    let parsed: f64 = ns_string::to_rust_string(env, string).trim().parse().unwrap_or(0.0);
    *env.objc.borrow_mut::<NSNumberHostObject>(this) = NSNumberHostObject::Double(parsed);
    this
}

@end

};
