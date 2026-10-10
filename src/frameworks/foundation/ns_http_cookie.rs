/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Minimal offline `NSHTTPCookie` and `NSHTTPCookieStorage` support.

use super::ns_array;
use super::NSUInteger;
use crate::dyld::{ConstantExports, HostConstant};
use crate::objc::{autorelease, id, msg, nil, objc_classes, ClassExports};

#[derive(Default)]
pub struct State {
    shared_storage: Option<id>,
}

pub const CONSTANTS: ConstantExports = &[
    (
        "_NSHTTPCookieDomain",
        HostConstant::NSString("NSHTTPCookieDomain"),
    ),
    (
        "_NSHTTPCookieName",
        HostConstant::NSString("NSHTTPCookieName"),
    ),
    (
        "_NSHTTPCookiePath",
        HostConstant::NSString("NSHTTPCookiePath"),
    ),
    (
        "_NSHTTPCookieValue",
        HostConstant::NSString("NSHTTPCookieValue"),
    ),
];

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSHTTPCookieStorage: NSObject

+ (id)sharedHTTPCookieStorage {
    if let Some(storage) = env.framework_state.foundation.ns_http_cookie.shared_storage {
        storage
    } else {
        let storage: id = msg![env; this new];
        env.framework_state.foundation.ns_http_cookie.shared_storage = Some(storage);
        storage
    }
}

- (id)cookiesForURL:(id)_url {
    // Network access is not backed by a real cookie jar in this emulator.
    let cookies = ns_array::from_vec(env, Vec::new());
    autorelease(env, cookies)
}

- (id)cookies {
    let cookies = ns_array::from_vec(env, Vec::new());
    autorelease(env, cookies)
}

- (NSUInteger)cookieAcceptPolicy {
    // NSHTTPCookieAcceptPolicyAlways
    0
}

- (())setCookieAcceptPolicy:(NSUInteger)_policy {}
- (())setCookie:(id)_cookie {}
- (())deleteCookie:(id)_cookie {}

- (())setCookies:(id)_cookies // NSArray<NSHTTPCookie *> *
             forURL:(id)_url // NSURL *
   mainDocumentURL:(id)_main_document_url {} // NSURL *

- (())deleteCookiesSinceDate:(id)_date {}

@end

@implementation NSHTTPCookie: NSObject

+ (id)cookieWithProperties:(id)_properties {
    // With no network cookie support, do not manufacture a cookie that would
    // appear to have been accepted by the remote server.
    nil
}

+ (id)cookiesWithResponseHeaderFields:(id)_headers forURL:(id)_url {
    let cookies = ns_array::from_vec(env, Vec::new());
    autorelease(env, cookies)
}

@end

};
