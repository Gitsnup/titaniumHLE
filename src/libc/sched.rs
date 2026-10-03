/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `sched.h`.

use crate::dyld::{export_c_func, FunctionExports};
use crate::Environment;

fn sched_yield(env: &mut Environment) -> i32 {
    log_dbg!(
        "TODO: thread {} requested processor yield, ignoring",
        env.current_thread
    );
    0 // success
}
fn sched_get_priority_max(env: &mut Environment, policy: i32) -> i32 {
    // This function only really matters for scheduling policies the guest can
    // actually use, which we don't emulate anyway. Just return plausible
    // values. (TODO: check the real ranges on iPhone OS)
    let _ = env;
    match policy {
        1 /* SCHED_OTHER */ | 2 /* SCHED_FIFO */ | 3 /* SCHED_RR */ => 127,
        _ => -1, // EINVAL
    }
}
fn sched_get_priority_min(env: &mut Environment, policy: i32) -> i32 {
    let _ = env;
    match policy {
        1 /* SCHED_OTHER */ | 2 /* SCHED_FIFO */ | 3 /* SCHED_RR */ => 0,
        _ => -1, // EINVAL
    }
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(sched_yield()),
    export_c_func!(sched_get_priority_max(_)),
    export_c_func!(sched_get_priority_min(_)),
];
