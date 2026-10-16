/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Grand Central Dispatch (`libdispatch`).
//!
//! Resources:
//! - Apple's [Grand Central Dispatch reference](https://developer.apple.com/documentation/dispatch)

use crate::abi::{CallFromHost, GuestFunction};
use crate::dyld::{export_c_func, FunctionExports};
use crate::mem::{MutPtr, MutVoidPtr};
use crate::Environment;

#[allow(non_camel_case_types)]
type dispatch_queue_t = MutVoidPtr;
#[allow(non_camel_case_types)]
type dispatch_time_t = i64;

/// Constant passed to [dispatch_time] to indicate "as soon as possible".
const DISPATCH_TIME_NOW: dispatch_time_t = 0;
/// Constant indicating a request should never run.
const DISPATCH_TIME_FOREVER: dispatch_time_t = i64::MAX;

/// Runs a block once, in a thread-safe manner.
///
/// TODO: proper synchronization for multi-threaded cases. For now this is
/// only safe on the main thread.
fn dispatch_once(env: &mut Environment, predicate: MutPtr<i32>, block: MutVoidPtr) {
    if predicate.is_null() || env.mem.read(predicate) != 0 {
        return;
    }
    env.mem.write(predicate, 1);
    log_dbg!("dispatch_once: invoking block {:?}", block);
    let block_flags: u32 = env.mem.read(block.cast::<u32>() + 1);
    // TODO: support C++-type blocks etc. if ever needed.
    const BLOCK_HAS_STRET: u32 = 0x20000000;
    assert!(block_flags & BLOCK_HAS_STRET == 0);
    // The block literal layout (32-bit):
    // struct Block_literal {
    //     void *isa; // 0
    //     int flags; // 4
    //     int reserved; // 8
    //     void (*invoke)(void *, ...); // 12
    //     struct Block_descriptor *descriptor; // 16
    // };
    let invoke: GuestFunction = env.mem.read(block.cast::<GuestFunction>() + 3);
    let (): () = invoke.call_from_host(env, (block,));
}

/// Converts a `dispatch_time_t` (relative or absolute) to a wall-clock time
/// in nanoseconds. Only the "relative to now" case is implemented; absolute
fn dispatch_time(
    env: &mut Environment,
    when: dispatch_time_t,
    seconds: i64,
    nanos: i64,
) -> dispatch_time_t {
    let _ = (env, when);
    // TODO: support DISPATCH_TIME_WALLCALENDAR etc.
    match when {
        DISPATCH_TIME_NOW => seconds
            .checked_mul(1_000_000_000)
            .and_then(|s| s.checked_add(nanos))
            .unwrap_or(i64::MAX),
        _ => todo!(),
    }
}

fn dispatch_after(
    env: &mut Environment,
    when: dispatch_time_t,
    queue: dispatch_queue_t,
    block: MutVoidPtr,
) {
    let _ = (queue, when);
    // FIXME: Arbitrarily maps to the main queue since concurrent queues are
    // not implemented.
    if when == DISPATCH_TIME_FOREVER {
        return;
    }
    // Simulate "after a delay" as "as soon as possible" on the main loop,
    // which matches how touchHLE treats other async work today.
    dispatch_once(env, MutPtr::null(), block)
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(dispatch_once(_, _)),
    export_c_func!(dispatch_time(_, _, _)),
    export_c_func!(dispatch_after(_, _, _)),
];
