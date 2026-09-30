/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CFHost` and the `CFHost`-related `CFStream` constructor

use crate::abi::GuestFunction;
use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::core_foundation::cf_allocator::{kCFAllocatorDefault, CFAllocatorRef};
use crate::frameworks::core_foundation::cf_string::CFStringRef;
use crate::frameworks::core_foundation::{CFIndex, CFOptionFlags, CFTypeRef};
use crate::frameworks::foundation::ns_string;
use crate::mem::{ConstPtr, MutPtr, MutVoidPtr};
use crate::objc::{msg, msg_class};
use crate::Environment;

// Note: on iOS SDK side this type is defined as a pointer to an opaque struct
type CFHostRef = CFTypeRef;

fn CFHostCreateWithName(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    name: CFStringRef,
) -> CFHostRef {
    assert!(allocator == kCFAllocatorDefault || env.mem.read(allocator).is_system_default()); // unimplemented
    log!(
        "TODO: CFHostCreateWithName('{}') -> opaque handle (no resolution)",
        ns_string::to_rust_string(env, name)
    );
    // No resolution is performed. A copied string is a safe stand-in for the
    // opaque CFHostRef: apps that import no other CFHost functions can only
    // retain, release, or pass the handle around.
    let copy: CFTypeRef = msg![env; name copy];
    copy
}

/// Creates a pair of stream objects. No connection is ever made: every
/// stream behaves as if the network were unreachable (Open fails, Read and
/// Write report errors), which is indistinguishable for a guest app from a
/// server it can no longer reach.
fn CFStreamCreatePairWithSocketToCFHost(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    host: CFHostRef,
    port: i32,
    read_stream: MutVoidPtr,
    write_stream: MutVoidPtr,
) {
    log!(
        "TODO: CFStreamCreatePairWithSocketToCFHost({:?}, port {}) -> dummy streams (no networking)",
        host,
        port
    );
    let read: CFTypeRef = msg_class![env; NSObject alloc];
    let write: CFTypeRef = msg_class![env; NSObject alloc];
    if !read_stream.is_null() {
        let read_stream: MutPtr<CFTypeRef> = read_stream.cast();
        env.mem.write(read_stream, read);
    }
    if !write_stream.is_null() {
        let write_stream: MutPtr<CFTypeRef> = write_stream.cast();
        env.mem.write(write_stream, write);
    }
}

/// Opening always fails, as if the connection could not be established.
fn CFReadStreamOpen(_env: &mut Environment, _stream: CFTypeRef) -> bool {
    log!("TODO: CFReadStreamOpen -> false (no networking)");
    false
}

fn CFReadStreamClose(_env: &mut Environment, _stream: CFTypeRef) {
    // Nothing to do: the stream was never open.
}

/// Reads always fail: the real API returns -1 on error.
fn CFReadStreamRead(
    _env: &mut Environment,
    _stream: CFTypeRef,
    _buffer: MutPtr<u8>,
    _max: CFIndex,
) -> CFIndex {
    log!("TODO: CFReadStreamRead -> -1 (no networking)");
    -1
}

fn CFReadStreamScheduleWithRunLoop(
    _env: &mut Environment,
    _stream: CFTypeRef,
    _run_loop: CFTypeRef,
    _mode: CFStringRef,
) {
    // No events will ever fire, so there is nothing to schedule.
}

fn CFReadStreamUnscheduleFromRunLoop(
    _env: &mut Environment,
    _stream: CFTypeRef,
    _run_loop: CFTypeRef,
    _mode: CFStringRef,
) {
    // Nothing to unschedule.
}

/// Opening always fails, as if the connection could not be established.
fn CFWriteStreamOpen(_env: &mut Environment, _stream: CFTypeRef) -> bool {
    log!("TODO: CFWriteStreamOpen -> false (no networking)");
    false
}

fn CFWriteStreamClose(_env: &mut Environment, _stream: CFTypeRef) {
    // Nothing to do: the stream was never open.
}

/// Writes always fail: the real API returns -1 on error.
fn CFWriteStreamWrite(
    _env: &mut Environment,
    _stream: CFTypeRef,
    _buffer: ConstPtr<u8>,
    _len: CFIndex,
) -> CFIndex {
    log!("TODO: CFWriteStreamWrite -> -1 (no networking)");
    -1
}

/// Accepts the client registration; no stream events will ever be sent since
/// the stream can never open.
fn CFWriteStreamSetClient(
    _env: &mut Environment,
    _stream: CFTypeRef,
    _stream_events: CFOptionFlags,
    _client_callback: GuestFunction, // TODO: CFWriteStreamClientCallBack
    _client_context: MutVoidPtr,     // TODO: CFStreamClientContext *
) -> bool {
    log!("TODO: CFWriteStreamSetClient -> true (no networking)");
    true
}

fn CFWriteStreamScheduleWithRunLoop(
    _env: &mut Environment,
    _stream: CFTypeRef,
    _run_loop: CFTypeRef,
    _mode: CFStringRef,
) {
    // No events will ever fire, so there is nothing to schedule.
}

fn CFWriteStreamUnscheduleFromRunLoop(
    _env: &mut Environment,
    _stream: CFTypeRef,
    _run_loop: CFTypeRef,
    _mode: CFStringRef,
) {
    // Nothing to unschedule.
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFHostCreateWithName(_, _)),
    export_c_func!(CFStreamCreatePairWithSocketToCFHost(_, _, _, _, _)),
    export_c_func!(CFReadStreamOpen(_)),
    export_c_func!(CFReadStreamClose(_)),
    export_c_func!(CFReadStreamRead(_, _, _)),
    export_c_func!(CFReadStreamScheduleWithRunLoop(_, _, _)),
    export_c_func!(CFReadStreamUnscheduleFromRunLoop(_, _, _)),
    export_c_func!(CFWriteStreamOpen(_)),
    export_c_func!(CFWriteStreamClose(_)),
    export_c_func!(CFWriteStreamWrite(_, _, _)),
    export_c_func!(CFWriteStreamSetClient(_, _, _, _)),
    export_c_func!(CFWriteStreamScheduleWithRunLoop(_, _, _)),
    export_c_func!(CFWriteStreamUnscheduleFromRunLoop(_, _, _)),
];
