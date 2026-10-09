/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSOperation` and `NSOperationQueue`.
//!
//! Operations are run synchronously on the calling thread when added to a
//! queue. This is sufficient for apps that just hand work to a queue for
//! later execution and don't depend on actual concurrency.

use crate::frameworks::foundation::NSUInteger;
use crate::objc::{id, msg, nil, objc_classes, retain, ClassExports, HostObject, NSZonePtr};

#[derive(Default)]
pub struct State {
    main_queue: id,
}

struct NSOperationHostObject {
    cancelled: bool,
    finished: bool,
    executing: bool,
}
impl HostObject for NSOperationHostObject {}

struct NSOperationQueueHostObject {
    operations: Vec<id>,
}
impl HostObject for NSOperationQueueHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSOperation: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSOperationHostObject {
        cancelled: false,
        finished: false,
        executing: false,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)init {
    // The host object was already created by allocWithZone:.
    this
}

- (())main {
    log_dbg!("TODO: [(NSOperation*) {:?} main] (base class no-op)", this);
}

- (())start {
    let host_obj = env.objc.borrow::<NSOperationHostObject>(this);
    if host_obj.cancelled || host_obj.finished || host_obj.executing {
        return;
    }
    env.objc.borrow_mut::<NSOperationHostObject>(this).executing = true;
    // Note: the guest may override `main`, or override `start` entirely (in
    // which case this base implementation won't be called).
    let _: () = msg![env; this main];
    let host_obj = env.objc.borrow_mut::<NSOperationHostObject>(this);
    host_obj.executing = false;
    host_obj.finished = true;
}

- (())cancel {
    env.objc.borrow_mut::<NSOperationHostObject>(this).cancelled = true;
}

- (bool)isCancelled {
    env.objc.borrow::<NSOperationHostObject>(this).cancelled
}

- (bool)isFinished {
    env.objc.borrow::<NSOperationHostObject>(this).finished
}

- (bool)isExecuting {
    env.objc.borrow::<NSOperationHostObject>(this).executing
}

- (bool)isReady {
    true
}

@end

@implementation NSOperationQueue: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSOperationQueueHostObject {
        operations: Vec::new(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)mainQueue {
    // Note: this is a lazy getter, not an allocator: all callers expect the
    // same instance for the main queue.
    if env.framework_state.foundation.ns_operation.main_queue == nil {
        let new: id = msg![env; this alloc];
        let new: id = msg![env; new init];
        env.framework_state.foundation.ns_operation.main_queue = new;
    }
    retain(env, env.framework_state.foundation.ns_operation.main_queue)
}

+ (id)currentQueue {
    // Note: guest threads have no queue association here.
    nil
}

- (id)init {
    // The host object was already created by allocWithZone:.
    this
}

- (())addOperation:(id)operation {
    // Operations run synchronously on the calling thread.
    let _: () = msg![env; operation start];
    let host_obj = env.objc.borrow_mut::<NSOperationQueueHostObject>(this);
    host_obj.operations.push(operation);
}

- (())addOperationWithBlock:(id)block {
    // Note: NSBlockOperation is not implemented; guest blocks are invoked
    // directly.
    let block: id = retain(env, block);
    // A block is a guest function; call it via msg is not possible, so this
    // is a no-op unless the guest uses addOperation: instead.
    log_dbg!("TODO: [(NSOperationQueue*) {:?} addOperationWithBlock: {:?}]", this, block);
}

- (())addOperations:(id)operations waitUntilFinished:(bool)_wait_until_finished {
    // Note: the array is iterated by index because it is an NSArray.
    let count: u32 = msg![env; operations count];
    for i in 0..count {
        let operation: id = msg![env; operations objectAtIndex:i];
        if operation != nil {
            let _: () = msg![env; operation start];
            let host_obj = env.objc.borrow_mut::<NSOperationQueueHostObject>(this);
            host_obj.operations.push(operation);
        }
    }
}

- (NSUInteger)operationCount {
    env.objc.borrow::<NSOperationQueueHostObject>(this).operations.len() as NSUInteger
}

- (())setMaxConcurrentOperationCount:(NSUInteger)_count {
    // Ignored: operations run synchronously.
}

- (())setName:(id)_name {
    // Ignored.
}

@end

};
