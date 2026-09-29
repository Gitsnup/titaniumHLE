/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIToolbar` and `UIBarButtonItem`.
//!
//! The main use case is toolbars deserialized from nib files, e.g. by apps
//! built from the Xcode utility-application template. Items are laid out
//! horizontally: bordered title buttons, fixed spacers of their given width
//! and flexible spacers sharing the leftover space.

use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect};
use crate::frameworks::foundation::ns_array;
use crate::frameworks::foundation::ns_string;
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_class, msg_send, msg_super, nil, objc_classes,
    release, retain, ClassExports, HostObject, NSZonePtr, SEL,
};
use crate::Environment;

use super::ui_control::ui_button::UIButtonTypeRoundedRect;
use super::ui_control::UIControlEventTouchUpInside;
use super::ui_control::UIControlStateNormal;

pub(crate) struct UIBarButtonItemHostObject {
    /// `NSString*`, or nil
    title: id,
    /// `UIBarButtonSystemItem` value if this is a system item
    system_item: Option<i32>,
    width: CGFloat,
    enabled: bool,
    style: i32,
    /// Weak reference, like `UIControl`'s targets.
    target: id,
    action: Option<SEL>,
}
impl HostObject for UIBarButtonItemHostObject {}
impl Default for UIBarButtonItemHostObject {
    fn default() -> Self {
        UIBarButtonItemHostObject {
            title: nil,
            system_item: None,
            width: 0.0,
            enabled: true,
            style: 0,
            target: nil,
            action: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct UIToolbarHostObject {
    superclass: super::UIViewHostObject,
    bar_style: i64,
    /// `UIBarButtonItem*`s
    items: Vec<id>,
    /// `UIButton*`s created to represent the items, one per tappable item.
    item_buttons: Vec<id>,
}
impl_HostObject_with_superclass!(UIToolbarHostObject);

/// `UIBarButtonSystemFlexibleSpace`, easier to read at the use site.
const SYSTEM_ITEM_FLEXIBLE_SPACE: i32 = 5;
/// `UIBarButtonSystemFixedSpace`.
const SYSTEM_ITEM_FIXED_SPACE: i32 = 6;

/// Releases a set of `UIButton`s created to represent bar button items.
///
/// Shared by `UIToolbar` and `UINavigationBar`, which both rebuild their
/// buttons whenever their items change.
pub(crate) fn release_bar_button_item_buttons(env: &mut Environment, buttons: Vec<id>) {
    for button in buttons {
        () = msg![env; button removeFromSuperview];
        release(env, button);
    }
}

/// Builds the `UIButton` that represents `item` in a bar, or returns nil for an
/// item that is not tappable (a spacer).
///
/// `max_x` is the right-hand edge the button must fit inside. `align_right`
/// positions the button against that edge instead of the left-hand `padding`,
/// for items at the right end of a bar. The caller retains the result and is
/// responsible for adding it as a subview, so that both bars can decide their
/// own stacking order.
pub(crate) fn layout_bar_button_item(
    env: &mut Environment,
    item: id,
    padding: CGFloat,
    max_x: CGFloat,
    height: CGFloat,
    align_right: bool,
) -> Option<id> {
    let (system_item, width, title) = {
        // NB: take copies and drop the borrow before any message sends.
        // Holding a borrow of the item across `msg![env; ...]` panics,
        // because the called method may borrow the same object.
        let host_obj = env.objc.borrow::<UIBarButtonItemHostObject>(item);
        (host_obj.system_item, host_obj.width, host_obj.title)
    };
    // Spacers are not tappable and have nothing to show.
    if system_item.is_some() {
        return None;
    }

    let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeRoundedRect];

    let mut button_width = width;
    if button_width <= 0.0 {
        let text = if title == nil {
            String::new()
        } else {
            ns_string::to_rust_string(env, title).into_owned()
        };
        // Rough estimate: enough for a default system font label.
        button_width = text.len() as CGFloat * 8.0 + 24.0;
    }

    let x = if align_right {
        (max_x - padding - button_width).max(padding)
    } else {
        padding
    };

    if title != nil {
        () = msg![env; button setTitle:title forState:UIControlStateNormal];
    }
    let title_color: id = msg_class![env; UIColor whiteColor];
    () = msg![env; button setTitleColor:title_color forState:UIControlStateNormal];
    () = msg![env; button setFrame:(CGRect {
        origin: CGPoint { x, y: 0.0 },
        size: crate::frameworks::core_graphics::CGSize { width: button_width, height },
    })];
    () = msg![env; button layoutSubviews];

    let action_sel: SEL = env
        .objc
        .lookup_selector("_touchHLE_barItemTouchUpInside")
        .unwrap_or_else(|| {
            env.objc
                .register_host_selector("_touchHLE_barItemTouchUpInside".to_string(), &mut env.mem)
        });
    () = msg![env; button addTarget:item
                            action:action_sel
                  forControlEvents:UIControlEventTouchUpInside];
    retain(env, button);
    Some(button)
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIBarButtonItem: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UIBarButtonItemHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())dealloc {
    let UIBarButtonItemHostObject { title, .. } = std::mem::take(env.objc.borrow_mut(this));
    release(env, title);
    env.objc.dealloc_object(this, &mut env.mem)
}

// NSCoding implementation
- (id)initWithCoder:(id)coder {
    let title_key = ns_string::get_static_str(env, "UITitle");
    let title: id = msg![env; coder decodeObjectForKey:title_key];
    if title != nil {
        retain(env, title);
    }

    let style_key = ns_string::get_static_str(env, "UIStyle");
    let style: i32 = msg![env; coder decodeIntForKey:style_key];

    let is_system_key = ns_string::get_static_str(env, "UIIsSystemItem");
    let is_system_item: bool = msg![env; coder decodeBoolForKey:is_system_key];

    let system_item_key = ns_string::get_static_str(env, "UISystemItem");
    let system_item: i32 = msg![env; coder decodeIntForKey:system_item_key];

    let width_key = ns_string::get_static_str(env, "UIWidth");
    let width: f64 = msg![env; coder decodeDoubleForKey:width_key];

    let enabled_key = ns_string::get_static_str(env, "UIEnabled");
    let enabled: bool = msg![env; coder decodeBoolForKey:enabled_key];

    let host_obj = env.objc.borrow_mut::<UIBarButtonItemHostObject>(this);
    host_obj.title = title;
    host_obj.style = style;
    host_obj.width = width as CGFloat;
    host_obj.enabled = enabled;
    host_obj.system_item = if is_system_item { Some(system_item) } else { None };

    this
}

- (id)initWithBarButtonSystemItem:(i32)system_item
                           target:(id)target
                           action:(SEL)action {
    let host_obj = env.objc.borrow_mut::<UIBarButtonItemHostObject>(this);
    host_obj.system_item = Some(system_item);
    host_obj.target = target;
    host_obj.action = Some(action);
    this
}

- (id)title {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).title
}

- (())setEnabled:(bool)enabled {
    env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).enabled = enabled;
}

- (bool)isEnabled {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).enabled
}

// Called by nib event connections and by code. The control event is ignored:
// items are treated as touch-up-inside buttons.
- (id)target {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).target
}

- (())setTarget:(id)target {
    env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).target = target;
}

- (SEL)action {
    // SEL in this emulator is a non-nullable pointer, so there is no null
    // selector to return for an item that has no action. `_cmd` doubles as a
    // harmless stand-in: the only caller that matters is the guest comparing
    // against the selector it just set.
    env.objc
        .borrow::<UIBarButtonItemHostObject>(this)
        .action
        .unwrap_or(_cmd)
}

// Unlike `addTarget:action:forControlEvents:`, the bare `setAction:` leaves the
// target alone. UIKit's `-setAction:` is documented to also clear the target if
// the selector has no colons and the target is the file's owner, but nib
// unarchiving sets target and action separately, so keep it a plain store.
- (())setAction:(SEL)action {
    env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).action = Some(action);
}

- (())addTarget:(id)target
         action:(SEL)action
forControlEvents:(u32)_control_events {
    let host_obj = env.objc.borrow_mut::<UIBarButtonItemHostObject>(this);
    host_obj.target = target;
    host_obj.action = Some(action);
}

// Private: sent by the button that represents this item in the toolbar.
- (())_touchHLE_barItemTouchUpInside {
    let UIBarButtonItemHostObject { target, action, .. } = *env.objc.borrow(this);
    if target == nil {
        return;
    }
    let Some(action) = action else { return; };
    let sel_str = action.as_str(&env.mem);
    let colon_count = sel_str.bytes().filter(|&b| b == b':').count();
    match colon_count {
        0 => {
            () = msg_send(env, (target, action));
        }
        1 => {
            () = msg_send(env, (target, action, this));
        }
        2 => {
            let event = nil;
            () = msg_send(env, (target, action, this, event));
        }
        _ => panic!("Unexpected action selector {sel_str}"),
    };
}

@end

@implementation UIToolbar: UIView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UIToolbarHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())dealloc {
    let UIToolbarHostObject {
        superclass: _,
        bar_style: _,
        items,
        item_buttons,
    } = std::mem::take(env.objc.borrow_mut(this));
    for item in items {
        release(env, item);
    }
    for button in item_buttons {
        release(env, button);
    }
    msg_super![env; this dealloc]
}

// NSCoding implementation
- (id)initWithCoder:(id)coder {
    let this: id = msg_super![env; this initWithCoder:coder];

    let style_key = ns_string::get_static_str(env, "UIBarStyle");
    let bar_style: i64 = msg![env; coder decodeInt64ForKey:style_key];

    let items_key = ns_string::get_static_str(env, "UIItems");
    let items: id = msg![env; coder decodeObjectForKey:items_key];

    env.objc.borrow_mut::<UIToolbarHostObject>(this).bar_style = bar_style;

    let items_vec: Vec<id> = if items == nil {
        Vec::new()
    } else {
        let count: u32 = msg![env; items count];
        (0..count)
            .map(|i| {
                let item: id = msg![env; items objectAtIndex:i];
                retain(env, item);
                item
            })
            .collect()
    };
    env.objc.borrow_mut::<UIToolbarHostObject>(this).items = items_vec;

    this
}

- (id)init {
    let this: id = msg_super![env; this init];
    // UIKit's default toolbar is a translucent black bar.
    let color: id = msg_class![env; UIColor blackColor];
    () = msg![env; this setBackgroundColor:color];
    this
}

- (id)items {
    let items = env.objc.borrow::<UIToolbarHostObject>(this).items.clone();
    ns_array::from_vec(env, items)
}

- (())setItems:(id)new_items { // NSArray<UIBarButtonItem *>*
    let new_items: Vec<id> = if new_items == nil {
        Vec::new()
    } else {
        let count: u32 = msg![env; new_items count];
        (0..count)
            .map(|i| {
                let item: id = msg![env; new_items objectAtIndex:i];
                retain(env, item);
                item
            })
            .collect()
    };
    let host_obj = env.objc.borrow_mut::<UIToolbarHostObject>(this);
    for item in std::mem::replace(&mut host_obj.items, new_items) {
        release(env, item);
    }
}

- (())layoutSubviews {
    // (Re)build the item buttons if the items changed, then position them.
    let items = env.objc.borrow::<UIToolbarHostObject>(this).items.clone();
    let old_buttons = std::mem::take(&mut env.objc.borrow_mut::<UIToolbarHostObject>(this).item_buttons);
    for button in old_buttons {
        () = msg![env; button removeFromSuperview];
        release(env, button);
    }

    let bounds: CGRect = msg![env; this bounds];
    if items.is_empty() || bounds.size.width <= 0.0 {
        return;
    }

    // Decide each item's width. Flexible spaces share the leftover space.
    let mut widths: Vec<CGFloat> = Vec::with_capacity(items.len());
    let mut flexible_count = 0;
    let mut used_width: CGFloat = 0.0;
    for &item in &items {
        let host_obj = env.objc.borrow::<UIBarButtonItemHostObject>(item);
        let width = match host_obj.system_item {
            Some(SYSTEM_ITEM_FLEXIBLE_SPACE) => {
                flexible_count += 1;
                0.0
            }
            Some(SYSTEM_ITEM_FIXED_SPACE) => host_obj.width,
            _ => {
                let title: id = msg![env; item title];
                let text = if title == nil {
                    String::new()
                } else {
                    ns_string::to_rust_string(env, title).into_owned()
                };
                // Rough estimate: enough for a default system font label.
                text.len() as CGFloat * 8.0 + 24.0
            }
        };
        used_width += width;
        widths.push(width);
    }

    let button_height: CGFloat = 30.0;
    let padding: CGFloat = 8.0;
    let mut flexible_width = 0.0;
    if flexible_count > 0 {
        let leftover = bounds.size.width - used_width - padding * (items.len() as CGFloat + 1.0);
        flexible_width = (leftover / flexible_count as CGFloat).max(20.0);
    }

    let bar_y = ((bounds.size.height - button_height) / 2.0).max(0.0);
    let mut x = padding;
    for (&item, &width) in items.iter().zip(widths.iter()) {
        // NB: take copies and drop the borrow before any message sends. Holding
        // a borrow of the item across `msg![env; ...]` panics, because the
        // called method may borrow the same object.
        let (system_item, is_tappable) = {
            let host_obj = env.objc.borrow::<UIBarButtonItemHostObject>(item);
            (host_obj.system_item, host_obj.system_item.is_none())
        };
        let item_width = if system_item == Some(SYSTEM_ITEM_FLEXIBLE_SPACE) {
            flexible_width
        } else {
            width
        };

        if is_tappable {
            let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeRoundedRect];
            let title: id = msg![env; item title];
            if title != nil {
                () = msg![env; button setTitle:title forState:UIControlStateNormal];
            }
            let title_color: id = msg_class![env; UIColor whiteColor];
            () = msg![env; button setTitleColor:title_color forState:UIControlStateNormal];
            () = msg![env; button setFrame:(CGRect {
                origin: CGPoint { x, y: bar_y },
                size: crate::frameworks::core_graphics::CGSize { width: item_width, height: button_height },
            })];
            () = msg![env; button layoutSubviews];

            let action_sel: SEL = env.objc
                .lookup_selector("_touchHLE_barItemTouchUpInside")
                .unwrap_or_else(|| {
                    env.objc
                        .register_host_selector("_touchHLE_barItemTouchUpInside".to_string(), &mut env.mem)
                });
            () = msg![env; button addTarget:item
                                    action:action_sel
                          forControlEvents:UIControlEventTouchUpInside];
            () = msg![env; this addSubview:button];
            // Own our reference so we can release it on the next layout.
            retain(env, button);
            env.objc.borrow_mut::<UIToolbarHostObject>(this).item_buttons.push(button);
        }

        x += item_width + padding;
    }
}

- (())setBarStyle:(i64)bar_style {
    env.objc.borrow_mut::<UIToolbarHostObject>(this).bar_style = bar_style;
}

- (i64)barStyle {
    env.objc.borrow::<UIToolbarHostObject>(this).bar_style
}

@end

};
