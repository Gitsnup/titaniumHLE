/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Stack-smashing protector support (`-fstack-protector`).
//!
//! Stack-protected guest code reads the canary from `___stack_chk_guard`
//! (indirectly: the non-lazy pointer slot holds the address of a word holding
//! the canary value) in function prologues and compares it again in
//! epilogues, calling `__stack_chk_fail` on mismatch. Since guest code can't
//! corrupt our guest memory layout by design, a fixed canary value is as good
//! as a random one.

use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::mem::{ConstVoidPtr, MutPtr};
use crate::Environment;

pub const CONSTANTS: ConstantExports = &[(
    "___stack_chk_guard",
    HostConstant::Custom(|env| -> ConstVoidPtr {
        let canary_ptr: MutPtr<u32> = env.mem.alloc_and_write(STACK_CHK_GUARD_VALUE);
        canary_ptr.cast().cast_const()
    }),
)];

/// The canary value that stack-protected guest code will see.
pub const STACK_CHK_GUARD_VALUE: u32 = 0x00c0ffee;

fn __stack_chk_fail(_env: &mut Environment) {
    panic!("__stack_chk_fail called: stack canary mismatch!");
}

pub const FUNCTIONS: FunctionExports = &[export_c_func!(__stack_chk_fail())];
