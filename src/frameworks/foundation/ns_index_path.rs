/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSIndexPath`.
//!
//! Only the row/section index paths used by `UITableView` are implemented.

use crate::frameworks::foundation::NSUInteger;
use crate::objc::{
    id, msg_class, objc_classes, ClassExports, HostObject, NSZonePtr,
};

#[derive(Default)]
struct NSIndexPathHostObject {
    row: NSUInteger,
    section: NSUInteger,
}
impl HostObject for NSIndexPathHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSIndexPath: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<NSIndexPathHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)indexPathForRow:(NSUInteger)row inSection:(NSUInteger)section {
    let new: id = msg_class![env; this alloc];
    let host_object = env.objc.borrow_mut::<NSIndexPathHostObject>(new);
    host_object.row = row;
    host_object.section = section;
    new
}

- (NSUInteger)row {
    env.objc.borrow::<NSIndexPathHostObject>(this).row
}

- (NSUInteger)section {
    env.objc.borrow::<NSIndexPathHostObject>(this).section
}

@end

};
