/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSUUID` is a wrapper around a 16-byte UUID.

use super::ns_string::from_rust_string;
use crate::objc::NSZonePtr;
use crate::objc::{id, msg, objc_classes, ClassExports, HostObject};

pub(super) struct UUIDHostObject {
    bytes: [u8; 16],
}
impl HostObject for UUIDHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSUUID: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(UUIDHostObject { bytes: [0u8; 16] });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)init {
    this
}

- (id)copyWithZone:(NSZonePtr)_zone {
    msg![env; this retain]
}

- (id)UUIDString {
    let bytes = &env.objc.borrow::<UUIDHostObject>(this).bytes;
    let uuid_string = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    );
    from_rust_string(env, uuid_string)
}

@end

};
