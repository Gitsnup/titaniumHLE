/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSAssertionHandler`.

use super::NSUInteger;
use crate::objc::{autorelease, id, msg, objc_classes, ClassExports, HostObject, NSZonePtr, SEL};

#[derive(Default)]
struct NSAssertionHandlerHostObject;
impl HostObject for NSAssertionHandlerHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSAssertionHandler: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    env.objc.alloc_object(
        this,
        Box::<NSAssertionHandlerHostObject>::default(),
        &mut env.mem,
    )
}

+ (id)currentHandler {
    let handler: id = msg![env; this alloc];
    autorelease(env, handler)
}

- (())handleFailureInMethod:(SEL)method
                     object:(id)_object
                       file:(id)_file // NSString *
                 lineNumber:(NSUInteger)line_number
                description:(id)_description, ..._args { // NSString *
    log!(
        "Ignoring guest assertion in method {} at line {}",
        method.as_str(&env.mem),
        line_number,
    );
}

- (())handleFailureInFunction:(id)_function_name // NSString *
                         file:(id)_file // NSString *
                   lineNumber:(NSUInteger)line_number
                  description:(id)_description, ..._args { // NSString *
    log!("Ignoring guest assertion at line {}", line_number);
}

@end

};
