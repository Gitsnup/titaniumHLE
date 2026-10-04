/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIAlertView`.

use crate::frameworks::foundation::ns_string;
use crate::objc::{
    id, impl_HostObject_with_superclass, msg_super, nil, objc_classes, release, retain,
    ClassExports, NSZonePtr,
};
use std::borrow::Cow;

struct UIAlertViewHostObject {
    superclass: super::UIViewHostObject,
    /// `NSString*`
    title: id,
    /// `NSString*`
    message: id,
    /// `id`, weak reference
    delegate: id,
}
impl_HostObject_with_superclass!(UIAlertViewHostObject);
impl Default for UIAlertViewHostObject {
    fn default() -> Self {
        Self {
            superclass: Default::default(),
            title: nil,
            message: nil,
            delegate: nil,
        }
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIAlertView: UIView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UIAlertViewHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithTitle:(id)title
                      message:(id)message
                     delegate:(id)delegate
            cancelButtonTitle:(id)cancelButtonTitle
            otherButtonTitles:(id)otherButtonTitles {

    log!("TODO: [(UIAlertView*){:?} initWithTitle:{:?} message:{:?} delegate:{:?} cancelButtonTitle:{:?} otherButtonTitles:{:?}]", this, title, message, delegate, cancelButtonTitle, otherButtonTitles);

    let message_str = if message == nil {
        Cow::from("(nil)")
    } else {
        ns_string::to_rust_string(env, message)
    };
    let title_str = if title == nil {
        Cow::from("(nil)")
    } else {
        ns_string::to_rust_string(env, title)
    };
    log!("UIAlertView: title: {:?}, message: {:?}", title_str, message_str);

    let this = msg_super![env; this init];
    if this != nil {
        let host_object = env.objc.borrow_mut::<UIAlertViewHostObject>(this);
        host_object.title = title;
        host_object.message = message;
        host_object.delegate = delegate;
        retain(env, title);
        retain(env, message);
    }
    this
}

- (id)title {
    let title = env.objc.borrow::<UIAlertViewHostObject>(this).title;
    retain(env, title);
    title
}

- (())setTitle:(id)title {
    let host_object = env.objc.borrow_mut::<UIAlertViewHostObject>(this);
    let old_title = std::mem::replace(&mut host_object.title, title);
    retain(env, title);
    release(env, old_title);
}

- (id)message {
    let message = env.objc.borrow::<UIAlertViewHostObject>(this).message;
    retain(env, message);
    message
}

- (())setMessage:(id)message {
    let host_object = env.objc.borrow_mut::<UIAlertViewHostObject>(this);
    let old_message = std::mem::replace(&mut host_object.message, message);
    retain(env, message);
    release(env, old_message);
}

- (id)delegate {
    env.objc.borrow::<UIAlertViewHostObject>(this).delegate
}

- (())setDelegate:(id)delegate {
    env.objc.borrow_mut::<UIAlertViewHostObject>(this).delegate = delegate;
}

- (())addButtonWithTitle:(id)title {
    log!("TODO: [(UIAlertView *){:?} addButtonWithTitle:{}]", this, ns_string::to_rust_string(env, title));
}

- (())show {
    log!("TODO: [(UIAlertView*){:?} show]", this);
}

- (())dealloc {
    let UIAlertViewHostObject {
        superclass: _,
        title,
        message,
        delegate: _,
    } = std::mem::take(env.objc.borrow_mut(this));
    release(env, title);
    release(env, message);
    msg_super![env; this dealloc]
}

@end

};
