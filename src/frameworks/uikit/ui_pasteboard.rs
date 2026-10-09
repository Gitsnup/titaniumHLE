/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIPasteboard`.

use crate::frameworks::foundation::ns_string;
use crate::objc::{
    id, msg, msg_class, msg_super, nil, objc_classes, retain, ClassExports, HostObject, NSZonePtr,
    ObjC,
};
use crate::Environment;
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    /// Maps pasteboard names to pasteboard objects. The name is an NSString,
    /// keyed by pointer.
    pasteboards: HashMap<id, id>,
}

struct UIPasteboardHostObject {
    /// The name of the pasteboard, or None for the general pasteboard.
    name: Option<String>,
}
impl HostObject for UIPasteboardHostObject {}

fn get_or_create_pasteboard(env: &mut Environment, name: Option<&'static str>) -> id {
    let name_string: id = match name {
        Some(name) => ns_string::get_static_str(env, name),
        None => msg_class![env; NSMutableString new],
    };
    if let Some(&existing) = env
        .framework_state
        .uikit
        .ui_pasteboard
        .pasteboards
        .get(&name_string)
    {
        return existing;
    }
    let host_object = Box::new(UIPasteboardHostObject {
        name: name.map(|n| n.to_string()),
    });
    let class = env.objc.get_known_class("UIPasteboard", &mut env.mem);
    let new = env.objc.alloc_object(class, host_object, &mut env.mem);
    retain(env, name_string);
    env.framework_state
        .uikit
        .ui_pasteboard
        .pasteboards
        .insert(name_string, new);
    new
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIPasteboard: NSObject

+ (id)generalPasteboard {
    get_or_create_pasteboard(env, Some("UIPasteboardNameGeneral"))
}

+ (id)pasteboardWithName:(id)name create:(bool)_create {
    if name == nil {
        return nil;
    }
    let name_str = ns_string::to_rust_string(env, name).into_owned();
    // Leak the name so it satisfies the 'static requirement of the string
    // pool; pasteboards are few and long-lived.
    let name_static: &'static str = Box::leak(name_str.into_boxed_str());
    get_or_create_pasteboard(env, Some(name_static))
}

+ (id)pasteboardWithUniqueName {
    // TODO: generate a unique name.
    log!("TODO: [(UIPasteboard*) {:?} pasteboardWithUniqueName] (using unnamed pasteboard)", this);
    get_or_create_pasteboard(env, None)
}

- (())dealloc {
    // TODO
    env.objc.dealloc_object(this, &mut env.mem)
}

// TODO: implement actual pasteboard content storage. For now, only the string
// convenience accessors are supported, and other content setters are no-ops.

- (())setPersistent:(bool)persistent {
    log!("TODO: [(UIPasteboard*) {:?} setPersistent:{:?}] (ignoring)", this, persistent);
}

- (())setString:(id)string {
    let _ = string;
}

- (id)string {
    nil
}

- (())setData:(id)data forPasteboardType:(id)pasteboard_type {
    log!("TODO: [(UIPasteboard*) {:?} setData:{:?} forPasteboardType:{:?}] (ignoring)", this, data, pasteboard_type);
}

- (())setValue:(id)value forPasteboardType:(id)pasteboard_type {
    log!("TODO: [(UIPasteboard*) {:?} setValue:{:?} forPasteboardType:{:?}] (ignoring)", this, value, pasteboard_type);
}

@end

};
