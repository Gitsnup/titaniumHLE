/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Minimal support for Objective-C blocks.
//!
//! Compiled Blocks literals (`^{ … }`) are anonymous structs whose first field
//! is an `isa` pointer to one of the NSConcrete*Block classes. Guest binaries
//! link these classes through the special `__NSConcreteStackBlock` /
//! `__NSConcreteGlobalBlock` / `__NSConcreteMallocBlock` symbols.
//!
//! We don't interpret the rest of the block layout, but pointing all three of
//! those symbols at a host-side `NSBlock` class means block pointers behave
//! like ordinary Objective-C objects for the messages that guest code sends to
//! them (mostly `retain`, `release`, `copy`, `autorelease`, and `class`).
//! `Block_copy`/`Block_release` are handled by the `_Block_object_*` helpers
//! and the guest's own copies of the block descriptor layout, so the only thing
//! that matters here is that `isa` resolves and memory management is a no-op
//! (the guest owns the literal's memory, so the runtime must never free it).
//!
//! Ref: <https://clang.llvm.org/docs/Block-ABI-Apple.html>
use crate::mem::ConstVoidPtr;
use crate::objc::{id, objc_classes, ClassExports, NSZonePtr, TrivialHostObject};
use crate::Environment;

pub const CLASSES: ClassExports = objc_classes! {
(env, this, _cmd);

@implementation NSBlock: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(TrivialHostObject);
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// A Blocks literal's memory is owned by guest code (it lives in the stack
// frame, or in static data for global blocks). The host runtime must not
// free it, so refcounting is a no-op.
- (())release {}
- (id)retain {
    this
}
- (id)copy {
    this
}
- (id)copyWithZone:(NSZonePtr)_zone {
    this
}

@end
};

/// Value of the `__NSConcrete*Block` symbols: the `NSBlock` class.
pub fn concrete_block_class(env: &mut Environment) -> ConstVoidPtr {
    env.objc
        .get_known_class("NSBlock", &mut env.mem)
        .cast()
        .cast_const()
}
