/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Stubs for the SJLJ-based exception unwinding runtime (`libgcc_s`).
//!
//! Apps built with GCC-era SDKs register these for `@try`/`@catch` and C++
//! exceptions. Actual unwinding is out of scope: apps that register handlers
//! but never throw work fine with no-op stubs.

use crate::dyld::FunctionExports;
use crate::mem::MutVoidPtr;
use crate::Environment;

fn Unwind_SjLj_Register(_env: &mut Environment, _context: MutVoidPtr) {}

fn Unwind_SjLj_Unregister(_env: &mut Environment, _context: MutVoidPtr) {}

pub const FUNCTIONS: FunctionExports = &[
    (
        "__Unwind_SjLj_Register",
        &(Unwind_SjLj_Register as fn(&mut Environment, MutVoidPtr)),
    ),
    (
        "__Unwind_SjLj_Unregister",
        &(Unwind_SjLj_Unregister as fn(&mut Environment, MutVoidPtr)),
    ),
];
