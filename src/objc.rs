/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Objective-C runtime.
//!
//! Apple's [Programming with Objective-C](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/Introduction/Introduction.html)
//! is a useful introduction to the language from a user's perspective.
//! There are further resources in the child modules of this module, but they
//! are more implementation-specific.
//!
//! The strategy for this emulator will be to provide our own implementations of
//! an Objective-C runtime and libraries for it (Foundation etc). These
//! implementations will be "host code": Rust code forming part of the emulator,
//! not emulated code. The runtime will need to be able to handle classes that
//! originate from the guest app, classes defined by the host, and sometimes
//! classes that are both (considering Objective-C's support for inheritance,
//! categories and dynamic class editing).

use crate::dyld::{
    export_c_func, export_c_func_aliased, ConstantExports, FunctionExports, HostConstant, HostDylib,
};
use crate::objc::messages::ThreadInitializer;
use crate::MutexId;
use std::collections::HashMap;

pub(super) mod classes;
mod messages;
mod methods;
mod objects;
mod properties;
mod selectors;
mod synchronization;

pub use classes::{objc_classes, Class, ClassExports, ClassTemplate};
pub use messages::{
    autorelease, msg, msg_class, msg_send, msg_send_no_initialize, msg_send_no_type_checking,
    msg_send_super2, msg_super, objc_super, release, retain,
};
pub use methods::{HostIMP, IMP};
pub use objects::{
    id, impl_HostObject_with_superclass, nil, AnyHostObject, HostObject, TrivialHostObject,
};
pub use properties::todo_objc_setter;
pub use selectors::{selector, SEL};

use crate::abi::{CallFromHost, GuestFunction};
use crate::mem::{ConstPtr, ConstVoidPtr, MutPtr, MutVoidPtr, Ptr};
use crate::Environment;
use classes::{
    class_addMethod, class_getInstanceMethod, class_getInstanceSize, class_getMethodImplementation,
    class_getProperty, class_getSuperclass, class_replaceMethod, objc_getClass, ClassHostObject,
    FakeClass, UnimplementedClass,
};
pub(crate) use messages::objc_msgSend;
use messages::{objc_msgSendSuper2, objc_msgSend_stret, MsgSendSignature, MsgSendSuperSignature};
use methods::{method_getImplementation, method_getTypeEncoding, method_list_t};
use objects::{objc_object, object_getClass, HostObjectEntry};
use properties::{ivar_list_t, objc_copyStruct, objc_getProperty, objc_setProperty};
use selectors::sel_registerName;
use synchronization::{objc_sync_enter, objc_sync_exit};

/// Typedef for `NSZone *`. This is a [fossil type] found in the signature of
/// `allocWithZone:` and similar methods. Its value is always ignored.
///
/// [fossil type]: https://en.wiktionary.org/wiki/fossil_word
pub type NSZonePtr = crate::mem::MutVoidPtr;

/// Main type holding Objective-C runtime state.
pub struct ObjC {
    /// Known selectors (interned method name strings).
    selectors: HashMap<String, SEL>,

    /// Mapping of known (guest) object pointers to their host objects.
    ///
    /// If an object isn't in this map, we will consider it not to exist.
    objects: HashMap<id, HostObjectEntry>,

    /// Ring buffer of the most recently deallocated object ids, for debugging
    /// use-after-dealloc message sends.
    recently_freed: std::collections::VecDeque<id>,

    /// Known classes.
    ///
    /// Look at the `isa` to get the metaclass for a class.
    classes: HashMap<String, Class>,

    /// Mutexes used in @synchronized blocks (objc_sync_enter/exit).
    sync_mutexes: HashMap<id, MutexId>,

    /// Mutexes for running the +initialize function.
    initializer_threads: HashMap<id, ThreadInitializer>,

    /// Pointer values of the dummy classes exported for block literals
    /// (`__NSConcreteStackBlock` and `__NSConcreteGlobalBlock`). Guest block
    /// literals use these as their `isa`, and must be messaged specially
    /// (see `objc_msgSend_inner`) rather than via normal class lookup.
    block_dummy_classes: Vec<u32>,

    /// Temporary storage for optional type information when sending a message.
    /// Type information isn't part of the `objc_msgSend` ABI, so an alternative
    /// channel is needed.
    message_type_info: Option<(std::any::TypeId, &'static str)>,
}

impl ObjC {
    pub fn new() -> ObjC {
        ObjC {
            selectors: HashMap::new(),
            objects: HashMap::new(),
            recently_freed: std::collections::VecDeque::new(),
            classes: HashMap::new(),
            sync_mutexes: HashMap::new(),
            initializer_threads: HashMap::new(),
            block_dummy_classes: Vec::new(),
            message_type_info: None,
        }
    }
}

pub const DYLIB: HostDylib = HostDylib {
    path: "/usr/lib/libobjc.A.dylib",
    aliases: &["/usr/lib/libobjc.dylib"],
    class_exports: &[],
    constant_exports: &[CONSTANTS],
    function_exports: &[FUNCTIONS],
};

fn get_dummy_object(env: &mut Environment) -> ConstVoidPtr {
    // A dummy non-null pointer for things that need *some* object to exist
    // (e.g. C++ exception typeinfo) but where we don't
    // (yet) implement the real thing.
    env.mem.alloc_and_write(0u32).cast().cast_const()
}

/// Like `get_dummy_object`, but also records the pointer as a block dummy
/// class so the message dispatcher can recognize block literals.
fn get_block_dummy_class(env: &mut Environment) -> ConstVoidPtr {
    let ptr = env.mem.alloc_and_write(0u32).cast().cast_const();
    env.objc.block_dummy_classes.push(ptr.to_bits());
    ptr
}

const CONSTANTS: ConstantExports = &[
    // We don't use these in our Objective-C runtime, but exporting useless
    // symbols for these silences the warning about the unhandled relocation,
    // and avoids a linker error for the integration tests.
    ("__objc_empty_vtable", HostConstant::NullPtr),
    ("__objc_empty_cache", HostConstant::NullPtr),
    (
        "__NSConcreteStackBlock",
        HostConstant::Custom(get_block_dummy_class),
    ),
    (
        "__NSConcreteGlobalBlock",
        HostConstant::Custom(get_block_dummy_class),
    ),
    ("_OBJC_EHTYPE_id", HostConstant::Custom(get_dummy_object)),
    (
        "_OBJC_EHTYPE_$_NSException",
        HostConstant::Custom(get_dummy_object),
    ),
];

/// Block support is iOS 4+, but it seems like Block Runtime Helpers
/// could still be called on even if minimal iOS version is set to 3.x?
///
/// ref. <https://clang.llvm.org/docs/Block-ABI-Apple.html#runtime-helper-functions>
/// Manual retain calls bypassing `objc_msgSend` (common in hand-written or
/// pre-ARC compiled code).
fn objc_retain(env: &mut Environment, object: id) -> id {
    retain(env, object)
}
/// Flag in a block literal's `flags` field indicating that the block has
/// copy/dispose helper functions in its descriptor.
/// See <https://clang.llvm.org/docs/Block-ABI-Apple.html>
const BLOCK_HAS_COPY_DISPOSE: u32 = 1 << 25;
const BLOCK_NEEDS_FREE: u32 = 1 << 24;

/// Retains (copies) a block literal and returns it.
///
/// Guest block literals are represented as ordinary objects with a dummy
/// class. A real `_Block_copy` heap-copies the literal and runs its copy
/// helper, which retains the captured objects via `_Block_object_assign`.
/// Skipping the copy helper means captured objects are never retained, so
/// they can be deallocated while the (now heap) block still references them
/// — observed as a use-after-free crash inside the Kiip SDK. To preserve
/// that semantic we run the guest's copy helper (with `dst == src`, so it
/// retains the captures in place) and then retain the literal itself.
fn objc_retainBlock(env: &mut Environment, block: id) -> id {
    if block == nil {
        return nil;
    }
    let base: u32 = block.to_bits();
    let flags_ptr: ConstPtr<u32> = Ptr::from_bits(base + 4);
    let flags: u32 = env.mem.read(flags_ptr);
    log_dbg!("retainBlock: block {:?} flags {:#x}", block, flags);
    if (flags & BLOCK_HAS_COPY_DISPOSE) != 0 && (flags & BLOCK_NEEDS_FREE) == 0 {
        let desc_ptr: ConstPtr<u32> = Ptr::from_bits(base + 16);
        let descriptor: MutPtr<u32> = Ptr::from_bits(env.mem.read(desc_ptr));
        let copy_helper_addr: u32 = env.mem.read(descriptor + 2);
        if copy_helper_addr != 0 {
            log_dbg!(
                "retainBlock: running copy helper {:#x} for {:?}",
                copy_helper_addr,
                block
            );
            let copy_helper =
                GuestFunction::from_addr_and_thumb_flag(copy_helper_addr & !0x1, true);
            let dst: MutVoidPtr = Ptr::from_bits(base);
            let () = copy_helper.call_from_host(env, (dst, dst));
        }
    }
    retain(env, block)
}

/// Assigns a block capture. Called by the guest's block copy helper for each
/// captured variable when a block is copied.
/// See <https://clang.llvm.org/docs/Block-ABI-Apple.html#runtime-helper-functions>
fn _Block_object_assign(env: &mut Environment, dest: MutVoidPtr, src: ConstVoidPtr, flags: i32) {
    const BLOCK_FIELD_IS_OBJECT: i32 = 0;
    const BLOCK_FIELD_IS_BLOCK: i32 = 1;
    const BLOCK_FIELD_IS_BYREF: i32 = 8;

    log_dbg!(
        "Block_object_assign: dest {:?} src {:?} flags {}",
        dest,
        src,
        flags
    );
    let dest: crate::mem::MutPtr<id> = Ptr::from_bits(dest.to_bits());
    if flags == BLOCK_FIELD_IS_OBJECT || flags == BLOCK_FIELD_IS_BLOCK {
        let object: id = Ptr::from_bits(src.to_bits());
        let retained: id = retain(env, object);
        env.mem.write(dest, retained);
    } else if flags == BLOCK_FIELD_IS_BYREF {
        // We don't model byref copies (see _Block_object_dispose), so just
        // pass the pointer through unmodified.
        let ptr: u32 = src.to_bits();
        env.mem.write(dest.cast(), ptr);
    } else {
        log!("Warning: unimplemented _Block_object_assign flags {flags}",);
        let ptr: u32 = src.to_bits();
        env.mem.write(dest.cast(), ptr);
    }
}

fn objc_retainAutoreleasedReturnValue(env: &mut Environment, object: id) -> id {
    retain(env, object)
}

fn objc_release(env: &mut Environment, object: id) {
    release(env, object)
}

fn objc_autorelease(env: &mut Environment, object: id) -> id {
    autorelease(env, object)
}

fn objc_autoreleaseReturnValue(env: &mut Environment, object: id) -> id {
    autorelease(env, object)
}
fn objc_retainAutorelease(env: &mut Environment, object: id) -> id {
    let object = retain(env, object);
    autorelease(env, object)
}

fn objc_retainAutoreleaseReturnValue(env: &mut Environment, object: id) -> id {
    let object = retain(env, object);
    autorelease(env, object)
}

/// ARC helper: `*object = value` with proper retain/release semantics.
/// See objc4's objc_storeStrong.
fn objc_storeStrong(env: &mut Environment, object: MutPtr<id>, value: id) {
    let old: id = env.mem.read(object);
    if value == old {
        return;
    }
    let new: id = if value != nil {
        retain(env, value)
    } else {
        nil
    };
    env.mem.write(object, new);
    if old != nil {
        release(env, old);
    }
}

fn _Block_object_dispose(_env: &mut Environment, object: ConstVoidPtr, flags: i32) {
    // `BLOCK_FIELD_IS_BYREF` flag defines an on stack structure holding
    // the __block variable. It is _probably_ safe to ignore.
    // TODO: properly implement for block support
    assert!(flags == 8); // BLOCK_FIELD_IS_BYREF
    log!(
        "Warning: Ignoring _Block_object_dispose({:?}, BLOCK_FIELD_IS_BYREF)",
        object
    );
}

const FUNCTIONS: FunctionExports = &[
    export_c_func!(class_getInstanceSize(_)),
    export_c_func!(class_getInstanceMethod(_, _)),
    export_c_func!(class_getSuperclass(_)),
    export_c_func!(class_getProperty(_, _)),
    export_c_func!(class_getMethodImplementation(_, _)),
    export_c_func!(class_replaceMethod(_, _, _, _)),
    export_c_func!(class_addMethod(_, _, _, _)),
    export_c_func!(method_getImplementation(_)),
    export_c_func!(method_getTypeEncoding(_)),
    export_c_func!(objc_msgSend(_, _)),
    export_c_func!(objc_msgSend_stret(_, _, _)),
    export_c_func!(objc_msgSendSuper2(_, _)),
    export_c_func!(objc_getClass(_)),
    export_c_func!(objc_getProperty(_, _, _, _)),
    export_c_func!(objc_setProperty(_, _, _, _, _, _)),
    export_c_func!(objc_copyStruct(_, _, _, _, _)),
    export_c_func!(objc_retain(_)),
    export_c_func!(objc_retainBlock(_)),
    export_c_func_aliased!("_Block_copy", objc_retainBlock(_)),
    export_c_func!(_Block_object_assign(_, _, _)),
    export_c_func!(objc_retainAutoreleasedReturnValue(_)),
    export_c_func!(objc_retainAutorelease(_)),
    export_c_func!(objc_release(_)),
    export_c_func!(objc_autorelease(_)),
    export_c_func!(objc_autoreleaseReturnValue(_)),
    export_c_func!(objc_retainAutoreleaseReturnValue(_)),
    export_c_func!(objc_sync_enter(_)),
    export_c_func!(objc_storeStrong(_, _)),
    export_c_func!(objc_sync_exit(_)),
    export_c_func!(NSAllocateObject(_, _, _)),
    export_c_func!(object_getClass(_)),
    export_c_func!(sel_registerName(_)),
    export_c_func_aliased!("sel_getUid", sel_registerName(_)),
    export_c_func!(_Block_object_dispose(_, _)),
];

/// C function `NSAllocateObject`: allocates an instance of `class` (with
/// optional `extraBytes` and `zone`). Equivalent to `+[class allocWithZone:]`,
/// so we just dispatch to that, which also picks the right host object type
/// for HLE classes.
fn NSAllocateObject(
    env: &mut Environment,
    class: Class,
    extra_bytes: u32,
    _zone: MutVoidPtr,
) -> id {
    assert!(
        extra_bytes == 0,
        "NSAllocateObject with extraBytes != 0 is unimplemented"
    );
    let sel = env
        .objc
        .lookup_selector("allocWithZone:")
        .expect("allocWithZone: selector missing");
    msg_send(env, (class, sel, MutVoidPtr::null()))
}
