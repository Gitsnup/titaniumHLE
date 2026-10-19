//! `NSOutputStream`.
//!
//! Minimal support for memory output streams, which the Kiip SDK uses to
//! buffer request bodies. Only the memory-target variant is implemented.

use crate::objc::{id, msg_class, nil, objc_classes, autorelease, ClassExports, HostObject};
use crate::mem::{ConstVoidPtr, MutVoidPtr};

/// The constant for the stream property that returns the written data,
/// exposed by NSStream as `NSStreamDataWrittenToMemoryStreamKey`.
const NS_STREAM_DATA_WRITTEN_TO_MEMORY_STREAM_KEY: &str = "NSStreamDataWrittenToMemoryStreamKey";

struct NSOutputStreamHostObject {
    buffer: Vec<u8>,
    closed: bool,
}
impl HostObject for NSOutputStreamHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSOutputStream: NSObject

+ (id)outputStreamToMemory {
    let host_object = Box::new(NSOutputStreamHostObject {
        buffer: Vec::new(),
        closed: false,
    });
    let new = env.objc.alloc_object(this, host_object, &mut env.mem);
    autorelease(env, new)
}

+ (id)outputStreamToBuffer:(MutVoidPtr)_buffer
                  capacity:(u32)_capacity {
    log!(
        "TODO: implement [NSOutputStream outputStreamToBuffer:capacity:] as a no-op"
    );
    nil
}

- (id)init {
    this
}

- (i32)write:(ConstVoidPtr)buffer
                      maxLength:(u32)length {
    if buffer.is_null() || length == 0 {
        return 0;
    }
    let bytes: &[u8] = env.mem.bytes_at(buffer.cast(), length);
    let bytes = bytes.to_vec();
    env.objc.borrow_mut::<NSOutputStreamHostObject>(this).buffer.extend(bytes);
    length as i32
}

- (())close {
    env.objc.borrow_mut::<NSOutputStreamHostObject>(this).closed = true;
}

- (id)propertyForKey:(id)key {
    let key_str = crate::frameworks::foundation::ns_string::to_rust_string(env, key);
    if key_str == NS_STREAM_DATA_WRITTEN_TO_MEMORY_STREAM_KEY {
        let buffer = env.objc.borrow_mut::<NSOutputStreamHostObject>(this).buffer.clone();
        let len = buffer.len() as u32;
        let guest_buffer = env.mem.alloc(len);
        env.mem
            .bytes_at_mut(guest_buffer.cast(), len)
            .copy_from_slice(&buffer[..]);
        let data: id = msg_class![env; NSData dataWithBytesNoCopy:guest_buffer length:len];
        data
    } else {
        nil
    }
}

- (())open {}
- (())scheduleInRunLoop:(id)_runLoop forMode:(id)_mode {}
- (())removeFromRunLoop:(id)_runLoop forMode:(id)_mode {}
- (id)delegate { nil }
- (())setDelegate:(id)_delegate {}
- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

};
