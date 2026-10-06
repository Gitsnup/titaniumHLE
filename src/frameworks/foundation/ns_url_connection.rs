/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSURLConnection`.

use super::{ns_string, NSInteger};
use crate::environment::Environment;
use crate::mem::MutPtr;
use crate::objc::{autorelease, id, msg, msg_class, nil, objc_classes, release, ClassExports};
use std::borrow::Cow;

const NSURLErrorDomain: &str = "NSURLErrorDomain";

/// Our helper type, Foundation just uses ints.
type NSURLErrorCode = NSInteger;
const NSURLErrorNotConnectedToInternet: NSURLErrorCode = -1009;

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSURLConnection: NSObject

+ (id)sendSynchronousRequest:(id)request // NSURLRequest *
           returningResponse:(MutPtr<id>)response // NSURLResponse **
                       error:(MutPtr<id>)out_error { // NSError **
    log!(
        "TODO: [NSURLConnection sendSynchronousRequest:{:?} ('{}') response:{:?} error:{:?}] -> empty",
        request,
        url_string_from_request(env, request),
        response,
        out_error,
    );
    if request == nil {
        if !response.is_null() {
            env.mem.write(response, nil);
        }
        if !out_error.is_null() {
            let domain = ns_string::get_static_str(env, NSURLErrorDomain);
            let error = msg_class![env; NSError alloc];
            // TODO: fill userInfo
            let error = msg![env; error initWithDomain:domain code:NSURLErrorNotConnectedToInternet userInfo:nil];
            autorelease(env, error);
            env.mem.write(out_error, error);
        }
        nil
    } else {
        if !response.is_null() {
            env.mem.write(response, nil);
        }
        if !out_error.is_null() {
            env.mem.write(out_error, nil);
        }
        // Return a valid, empty (NUL-terminated) NSData rather than nil, as
        // some apps assume the response is always valid.
        let nul_byte: MutPtr<u8> = env.mem.alloc_and_write(0u8);
        let nul_byte_void = nul_byte.cast_const().cast_void();
        let data: id = msg_class![env; NSData dataWithBytes:nul_byte_void length:(1u32)];
        autorelease(env, data)
    }
}

+ (id)connectionWithRequest:(id)request // NSURLRequest *
                   delegate:(id)delegate {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithRequest:request delegate:delegate];
    autorelease(env, new)
}

- (id)initWithRequest:(id)request // NSURLRequest *
             delegate:(id)delegate {
    msg![env; this initWithRequest:request delegate:delegate startImmediately:true]
}

- (id)initWithRequest:(id)request // NSURLRequest *
             delegate:(id)delegate
     startImmediately:(bool)start_immediately {
    log!(
        "TODO: [(NSURLConnection *){:?} initWithRequest:{:?} ('{}') delegate:{:?} startImmediately:{}] -> nil",
        this,
        request,
        url_string_from_request(env, request),
        delegate,
        start_immediately,
    );
    release(env, this);
    nil
}

@end

};

fn url_string_from_request(env: &mut Environment, request: id) -> Cow<'static, str> {
    if request == nil {
        Cow::from("(null)")
    } else {
        let url = msg![env; request URL];
        let ns_string = msg![env; url absoluteString];
        ns_string::to_rust_string(env, ns_string)
    }
}
