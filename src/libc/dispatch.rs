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

pub const FUNCTIONS: FunctionExports = &[export_c_func!(dispatch_once(_, _))];
