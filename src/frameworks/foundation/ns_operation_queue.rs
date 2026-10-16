/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSOperationQueue` and `NSOperation` (minimal, synchronous implementations).
//!
//! The guest app submits operations (or blocks) to a queue; we simply run them
//! on the main thread at the next opportunity (when the queue is not suspended
//! and `addOperation` is called). This is very far from real concurrent
//! behaviour, but it is enough for apps that merely offload work to a queue and
//! expect it to happen eventually, on the main thread.

use crate::frameworks::foundation::NSInteger;
use crate::objc::{id, msg, objc_classes, release, ClassExports, NSZonePtr};
use crate::Environment;

#[derive(Default)]
struct NSOperationQueueHostObject {
    suspended: bool,
}
impl crate::objc::HostObject for NSOperationQueueHostObject {}

#[derive(Default)]
struct NSOperationHostObject {
    done: bool,
}
impl crate::objc::HostObject for NSOperationHostObject {}

fn queue_was_initialized(env: &Environment, this: id) -> bool {
    env.objc
        .get_host_object(this)
        .and_then(|host_object| {
            host_object
                .as_any()
                .downcast_ref::<NSOperationQueueHostObject>()
        })
        .is_some()
}

/// Treats an uninitialized instance (e.g. one created via plain `[alloc]`)
/// as not suspended, which is the default.
fn queue_suspended(env: &Environment, this: id) -> bool {
    env.objc
        .get_host_object(this)
        .and_then(|host_object| {
            host_object
                .as_any()
                .downcast_ref::<NSOperationQueueHostObject>()
        })
        .map(|queue| queue.suspended)
        .unwrap_or(false)
}

fn op_was_initialized(env: &Environment, this: id) -> bool {
    env.objc
        .get_host_object(this)
        .and_then(|host_object| host_object.as_any().downcast_ref::<NSOperationHostObject>())
        .is_some()
}

fn op_is_done(env: &Environment, this: id) -> bool {
    env.objc
        .get_host_object(this)
        .and_then(|host_object| host_object.as_any().downcast_ref::<NSOperationHostObject>())
        .map(|op| op.done)
        .unwrap_or(false)
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSOperation: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    // Ensure the host object exists even if the app skips -init (or uses a
    // custom initializer we do not implement).
    env.objc.alloc_object(this, Box::new(NSOperationHostObject::default()), &mut env.mem)
}

- (id)init {
    this
}

- (bool)isReady {
    true
}

- (bool)isExecuting {
    false
}

- (bool)isFinished {
    // Be tolerant of instances that were never `-init`-ed.
    op_is_done(env, this)
}

- (())setCompletionBlock:(id)block {
    // TODO: support completion blocks
}

- (())start {
    // Operations without an override are no-ops.
    if op_was_initialized(env, this) {
        env.objc.borrow_mut::<NSOperationHostObject>(this).done = true;
    } else {
        log!("TODO: NSOperation ({:?}) was not initialized; ignoring start", this);
    }
}

- (())main {
}

- (())cancel {
}

- (bool)isCancelled {
    false
}

@end

@implementation NSOperationQueue: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    // Ensure the host object exists even if the app skips -init (or uses a
    // custom initializer we do not implement).
    env.objc.alloc_object(this, Box::new(NSOperationQueueHostObject::default()), &mut env.mem)
}

- (id)init {
    this
}

- (())addOperation:(id)operation { // NSOperation*
    let suspended = queue_suspended(env, this);
    if !suspended {
        let _: () = msg![env; operation start];
    }
    release(env, operation);
}

- (())addOperationWithBlock:(id)block {
    // TODO: support blocks; treat as a no-op for now.
    release(env, block);
}

- (())addOperations:(id)operations waitUntilDone:(bool)wait {
    // TODO
}

- (())setMaxConcurrentOperationCount:(NSInteger)count {
}

- (())setSuspended:(bool)suspended {
    if queue_was_initialized(env, this) {
        env.objc.borrow_mut::<NSOperationQueueHostObject>(this).suspended = suspended;
    } else {
        log!("TODO: NSOperationQueue ({:?}) was not initialized; ignoring setSuspended", this);
    }
}

- (bool)isSuspended {
    queue_suspended(env, this)
}

- (())cancelAllOperations {
}

- (())waitUntilAllOperationsAreFinished {
}

@end

};
