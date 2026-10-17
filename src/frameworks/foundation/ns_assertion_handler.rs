/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSAssertionHandler`.

use crate::frameworks::foundation::ns_string;
use crate::frameworks::foundation::NSInteger;
use crate::objc::{id, msg, objc_classes, ClassExports, HostObject, NSZonePtr, SEL};

#[derive(Default)]
struct NSAssertionHandlerHostObject;
impl HostObject for NSAssertionHandlerHostObject {}

#[derive(Default)]
pub struct State {
    /// Thread-local `NSAssertionHandler*` singleton.
    // TODO: NSAssertionHandler is documented as thread-local; we use a single
    // instance for simplicity.
    current_handler: Option<id>,
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSAssertionHandler: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<NSAssertionHandlerHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)currentHandler {
    if let Some(existing) = env.framework_state.foundation.ns_assertion_handler.current_handler {
        existing
    } else {
        let new: id = msg![env; this alloc];
        // TODO: -init is not implemented, but it doesn't matter for the
        // current host object.
        env.framework_state.foundation.ns_assertion_handler.current_handler = Some(new);
        new
    }
}

// These methods are called when an assertion fails. On a real system they
// raise NSInternalInconsistencyException. For now, just log the failure so
// that apps using assertions as sanity checks can keep running.
- (())handleFailureInMethod:(SEL)selector
                     object:(id)object
                       file:(id)file_name
                 lineNumber:(NSInteger)line
                description:(id)format_string, ...args {
    let file = ns_string::to_rust_string(env, file_name);
    log!(
        "Assertion failure in method {} {:?} ({}:{})",
        selector.as_str(&env.mem),
        object,
        file,
        line
    );
    // TODO: raise NSInternalInconsistencyException
    // TODO: format the description using the variadic arguments
}

- (())handleFailureInFunction:(id)function_name
                         file:(id)file_name
                   lineNumber:(NSInteger)line
                  description:(id)format_string, ...args {
    let file = ns_string::to_rust_string(env, file_name);
    log!(
        "Assertion failure in function {} ({}:{})",
        ns_string::to_rust_string(env, function_name),
        file,
        line
    );
    // TODO: raise NSInternalInconsistencyException
    // TODO: format the description using the variadic arguments
}

@end

};
