/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! The AdSupport framework.
//!
//! Apps that show ads use this to opt in or out of ad tracking. For a
//! well-behaved app, reporting that tracking is disabled (which is what we
//! do here) should be sufficient for it to skip ad-related code paths.

use crate::dyld::HostDylib;
use crate::objc::{id, msg, msg_class, objc_classes, ClassExports};

pub const DYLIB: HostDylib = HostDylib {
    path: "/System/Library/Frameworks/AdSupport.framework/AdSupport",
    aliases: &[],
    class_exports: &[CLASSES],
    constant_exports: &[],
    function_exports: &[],
};

const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation ASIdentifierManager: NSObject

+ (id)sharedManager {
    let static_class: id = msg_class![env; ASIdentifierManager class];
    let instance: id = msg![env; static_class alloc];
    msg![env; instance init]
}

- (id)advertisingIdentifier {
    // The all-zero ("nil") UUID. Apps that merely log or hash this value
    // will get a deterministic, non-identifying result.
    let uuid: id = msg_class![env; NSUUID alloc];
    msg![env; uuid init]
}

- (bool)isAdvertisingTrackingEnabled {
    false
}

@end

};
