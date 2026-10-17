/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSHTTPCookieStorage`.

use crate::frameworks::foundation::ns_array;
use crate::objc::{id, msg, nil, objc_classes, ClassExports, HostObject};

#[derive(Default)]
pub struct State {
    /// Shared `NSHTTPCookieStorage*` singleton.
    shared: Option<id>,
}

#[derive(Default)]
struct NSHTTPCookieStorageHostObject {
    cookies: Vec<id>,
}
impl HostObject for NSHTTPCookieStorageHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSHTTPCookieStorage: NSObject

+ (id)sharedHTTPCookieStorage {
    if let Some(existing) = env.framework_state.foundation.ns_http_cookie_storage.shared {
        existing
    } else {
        let host_object = Box::<NSHTTPCookieStorageHostObject>::default();
        let new: id = env.objc.alloc_object(this, host_object, &mut env.mem);
        env.framework_state.foundation.ns_http_cookie_storage.shared = Some(new);
        new
    }
}

- (())setCookie:(id)cookie {
    env.objc
        .borrow_mut::<NSHTTPCookieStorageHostObject>(this)
        .cookies
        .push(cookie);
}

- (id)cookies {
    let cookies = env.objc.borrow::<NSHTTPCookieStorageHostObject>(this).cookies.clone();
    ns_array::from_vec(env, cookies)
}

- (id)cookiesForURL:(id)_url {
    // TODO: filter by URL; the app only checks for emptiness
    msg![env; this cookies]
}

// TODO: handle more methods
- (id)methodSignatureForSelector:(id)_sel {
    nil
}

@end

};
