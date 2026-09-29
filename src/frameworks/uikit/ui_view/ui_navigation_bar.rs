/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UINavigationBar` and `UINavigationItem`.
//!
//! The main use case is navigation bars deserialized from nib files, e.g. in
//! flipside-style views, and bars managed by `UINavigationController`. A bar
//! owns a stack of `UINavigationItem`s and displays only the top one: its
//! title in the middle, its left and right bar button items at the edges.
//!
//! Only the top item is rendered. `pushNavigationItem:animated:` and
//! `popNavigationItemAnimated:` maintain the stack so that code can read it
//! back, but there is no transition animation.

use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_array;
use crate::frameworks::foundation::ns_string;
use crate::frameworks::foundation::NSUInteger;
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_class, msg_super, nil, objc_classes, release,
    retain, ClassExports, HostObject, NSZonePtr,
};

use super::ui_toolbar::{layout_bar_button_item, release_bar_button_item_buttons};
use super::UIViewHostObject;
use crate::Environment;

/// `UIBarStyleBlackOpaque`, the default for a bar created in code.
const BAR_STYLE_DEFAULT: i64 = 1;
/// `UIBarStyleBlackTranslucent`, which `UIView`'s nib decoding would otherwise
/// leave a bar at. It renders the same as black opaque here.
const BAR_STYLE_BLACK_TRANSLUCENT: i64 = 2;
/// `UIBarMetricsDefault`.
const BAR_METRICS_DEFAULT: i64 = 0;
/// Height of a bar using default metrics, excluding any status bar.
const BAR_HEIGHT: CGFloat = 44.0;

#[derive(Default)]
pub(crate) struct UINavigationItemHostObject {
    /// `NSString*`, or nil
    title: id,
    /// `UIView*` shown in place of the title, or nil
    title_view: id,
    /// `UIBarButtonItem*`s, or nil
    left_bar_button_item: id,
    right_bar_button_item: id,
    back_bar_button_item: id,
    /// The bar this item is currently on, or nil. Weak, and set by the bar.
    /// `UINavigationBar*`
    navigation_bar: id,
    /// Whether tapping the back button pops a view controller.
    /// Kept for interface compatibility; not currently acted on.
    hides_back_button: bool,
    /// Whether the title is replaced by the back button's title when the item
    /// is not the top item. Kept for interface compatibility.
    left_items_supplement_back_button: bool,
}
impl HostObject for UINavigationItemHostObject {}

#[derive(Default)]
pub(crate) struct UINavigationBarHostObject {
    superclass: UIViewHostObject,
    bar_style: i64,
    bar_metrics: i64,
    translucent: bool,
    /// `UINavigationItem*`s on this bar, bottom of the stack first.
    /// Non-retaining, because items retain their bar: retaining both ways
    /// would make the pair immortal.
    items: Vec<id>,
    /// The item this bar was initialized with, when that item was not pushed
    /// onto the stack. Retained. See `initWithCoder:`.
    pending_item: id,
    /// `UIButton*`s created to represent the top item's bar button items, one
    /// per side that has one.
    item_buttons: Vec<id>,
    /// `UILabel*` created to show the top item's title, if it has one.
    title_label: id,
    /// `UIView*` created to show the top item's title view, if it has one.
    title_view: id,
}
impl_HostObject_with_superclass!(UINavigationBarHostObject);

/// `UIBarMetrics` for the height of a bar.
fn bar_height(bar_metrics: i64) -> CGFloat {
    // Landscape phone metrics are 32pt; the other two are 44pt.
    if bar_metrics == 2 {
        32.0
    } else {
        BAR_HEIGHT
    }
}

/// Detaches and releases the buttons and labels showing `bar`'s top item.
///
/// Must be called with none of the bar's host object borrowed, because it
/// sends messages, and before the top item changes, so the old item is the
/// one torn down.
fn tear_down_top_item_views(env: &mut Environment, bar: id) {
    let old_buttons = std::mem::take(
        &mut env
            .objc
            .borrow_mut::<UINavigationBarHostObject>(bar)
            .item_buttons,
    );
    release_bar_button_item_buttons(env, old_buttons);

    let (title_label, title_view) = {
        let host_obj = env.objc.borrow_mut::<UINavigationBarHostObject>(bar);
        (
            std::mem::replace(&mut host_obj.title_label, nil),
            std::mem::replace(&mut host_obj.title_view, nil),
        )
    };
    for view in [title_label, title_view] {
        if view != nil {
            () = msg![env; view removeFromSuperview];
            release(env, view);
        }
    }
}

/// The item this bar should display: the top of the stack, falling back to the
/// item it was initialized with when nothing was ever pushed.
fn top_item(env: &mut Environment, bar: id) -> id {
    let (pushed, pending) = {
        let host_obj = env.objc.borrow::<UINavigationBarHostObject>(bar);
        (
            host_obj.items.last().copied().unwrap_or(nil),
            host_obj.pending_item,
        )
    };
    if pushed != nil {
        pushed
    } else {
        pending
    }
}

/// Sets the details of `label` from `props` and positions it in `frame`.
///
/// Used for both the title and bar button titles, which differ only in font
/// size and alignment.
fn configure_label(env: &mut Environment, label: id, props: &LabelProps, frame: CGRect) {
    let font_size = props.font_size;
    let font: id = msg_class![env; UIFont systemFontOfSize:font_size];
    () = msg![env; label setFont:font];
    let has_shadow = props.shadow_color != nil;
    let text_color: id = if has_shadow {
        msg_class![env; UIColor whiteColor]
    } else {
        msg_class![env; UIColor blackColor]
    };
    () = msg![env; label setTextColor:text_color];
    let shadow: id = props.shadow_color;
    let shadow_offset = props.shadow_offset;
    let alignment = props.alignment;
    () = msg![env; label setShadowColor:shadow];
    () = msg![env; label setShadowOffset:shadow_offset];
    () = msg![env; label setTextAlignment:alignment];
    () = msg![env; label setBackgroundColor:nil];
    let text = props.text;
    let number_of_lines = props.number_of_lines;
    () = msg![env; label setText:text];
    () = msg![env; label setNumberOfLines:number_of_lines];
    () = msg![env; label setFrame:frame];
    () = msg![env; label layoutSubviews];
}

/// Everything a bar label needs beyond its text and frame.
struct LabelProps {
    font_size: CGFloat,
    alignment: i32,
    /// nil for no shadow. A non-nil shadow also selects white text, which is
    /// what the bars this emulates do.
    shadow_color: id,
    shadow_offset: CGSize,
    number_of_lines: i32,
    text: id,
}

/// Builds the label and side button for `item` and lays them out in `bounds`.
///
/// The bar stacks its subviews, so a title label is created for every item
/// drawn and torn down again on the next item change. That is a little
/// wasteful but keeps this stateless, and matches what the real bar's layout
/// does often enough.
fn layout_top_item(env: &mut Environment, bar: id, item: id, bounds: CGRect) {
    let height = bounds.size.height;
    let padding: CGFloat = 8.0;

    let (title, title_view, left_item, right_item): (id, id, id, id) = {
        let host_obj = env.objc.borrow::<UINavigationItemHostObject>(item);
        (
            host_obj.title,
            host_obj.title_view,
            host_obj.left_bar_button_item,
            host_obj.right_bar_button_item,
        )
    };

    // bar_style decides the labels' colors: a black bar gets white text with a
    // dark shadow, a translucent black bar the same, the default (grey) style
    // darker text. touchHLE draws the bar's background as a solid color, so
    // the labels are what make the bar look right.
    let shadow_color: id = msg_class![env; UIColor colorWithWhite:0.0 alpha:0.5];

    // Left and right bar button items are drawn first, so a long title is
    // drawn under them rather than over.
    let mut buttons: Vec<id> = Vec::new();
    for (bar_item, side) in [(left_item, 0), (right_item, 1)] {
        if bar_item == nil {
            continue;
        }
        let Some(button) =
            layout_bar_button_item(env, bar_item, padding, bounds.size.width, height, side == 1)
        else {
            continue;
        };
        () = msg![env; bar addSubview:button];
        buttons.push(button);
    }

    if title_view != nil {
        // An explicit title view owns its own frame; only its position along
        // the bar is ours. Centre it and leave its size alone.
        let title_view_frame: CGRect = msg![env; title_view frame];
        let centered = CGRect {
            origin: CGPoint {
                x: (bounds.size.width - title_view_frame.size.width) / 2.0,
                y: (height - title_view_frame.size.height) / 2.0,
            },
            size: title_view_frame.size,
        };
        () = msg![env; title_view setFrame:centered];
        () = msg![env; bar addSubview:title_view];
    } else if title != nil {
        let label: id = msg_class![env; UILabel alloc];
        let label: id = msg![env; label initWithFrame:(<CGRect as Default>::default())];
        configure_label(
            env,
            label,
            &LabelProps {
                font_size: 20.0,
                alignment: 1, // UITextAlignmentCenter
                shadow_color,
                shadow_offset: CGSize {
                    width: 0.0,
                    height: -1.0,
                },
                number_of_lines: 1,
                text: title,
            },
            CGRect {
                origin: CGPoint { x: padding, y: 0.0 },
                size: CGSize {
                    width: bounds.size.width - padding * 2.0,
                    height,
                },
            },
        );
        () = msg![env; bar addSubview:label];
        let host_obj = env.objc.borrow_mut::<UINavigationBarHostObject>(bar);
        host_obj.title_label = label;
    }

    env.objc
        .borrow_mut::<UINavigationBarHostObject>(bar)
        .item_buttons = buttons;
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UINavigationItem: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UINavigationItemHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())dealloc {
    let UINavigationItemHostObject {
        title,
        title_view,
        left_bar_button_item,
        right_bar_button_item,
        back_bar_button_item,
        ..
    } = std::mem::take(env.objc.borrow_mut(this));
    for obj in [title, title_view, left_bar_button_item, right_bar_button_item, back_bar_button_item] {
        release(env, obj);
    }
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)initWithTitle:(id)title { // NSString *
    retain(env, title);
    env.objc.borrow_mut::<UINavigationItemHostObject>(this).title = title;
    this
}

// NSCoding implementation
- (id)initWithCoder:(id)coder {
    let title_key = ns_string::get_static_str(env, "UITitle");
    let title: id = msg![env; coder decodeObjectForKey:title_key];
    if title != nil {
        retain(env, title);
    }

    // NB: `UINavigationBar` here is the bar this item belongs to, decoded
    // before the item because the bar's `UIItems` array points at the item.
    let bar_key = ns_string::get_static_str(env, "UINavigationBar");
    let bar: id = msg![env; coder decodeObjectForKey:bar_key];

    let left_key = ns_string::get_static_str(env, "UILeftBarButtonItem");
    let left_item: id = msg![env; coder decodeObjectForKey:left_key];
    if left_item != nil {
        retain(env, left_item);
    }

    let right_key = ns_string::get_static_str(env, "UIRightBarButtonItem");
    let right_item: id = msg![env; coder decodeObjectForKey:right_key];
    if right_item != nil {
        retain(env, right_item);
    }

    {
        let host_obj = env.objc.borrow_mut::<UINavigationItemHostObject>(this);
        host_obj.title = title;
        host_obj.navigation_bar = bar;
        host_obj.left_bar_button_item = left_item;
        host_obj.right_bar_button_item = right_item;
    }

    // The bar's `UIItems` is decoded as a plain array of items that are not
    // pushed onto its stack, so register this item with its bar directly.
    log!("[UINavigationItem initWithCoder] self={:?} bar={:?}", this, bar);

    this
}

- (id)title {
    env.objc.borrow::<UINavigationItemHostObject>(this).title
}
- (())setTitle:(id)title { // NSString *
    let old_title = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationItemHostObject>(this).title, title);
    retain(env, title);
    release(env, old_title);
}
- (id)titleView {
    env.objc.borrow::<UINavigationItemHostObject>(this).title_view
}
- (())setTitleView:(id)title_view { // UIView *
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationItemHostObject>(this).title_view, title_view);
    retain(env, title_view);
    release(env, old);
}
- (id)leftBarButtonItem {
    env.objc.borrow::<UINavigationItemHostObject>(this).left_bar_button_item
}
- (())setLeftBarButtonItem:(id)item { // UIBarButtonItem *
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationItemHostObject>(this).left_bar_button_item, item);
    retain(env, item);
    release(env, old);
}
- (id)rightBarButtonItem {
    env.objc.borrow::<UINavigationItemHostObject>(this).right_bar_button_item
}
- (())setRightBarButtonItem:(id)item { // UIBarButtonItem *
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationItemHostObject>(this).right_bar_button_item, item);
    retain(env, item);
    release(env, old);
}
- (id)backBarButtonItem {
    env.objc.borrow::<UINavigationItemHostObject>(this).back_bar_button_item
}
- (())setBackBarButtonItem:(id)item { // UIBarButtonItem *
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationItemHostObject>(this).back_bar_button_item, item);
    retain(env, item);
    release(env, old);
}
- (id)navigationBar {
    env.objc.borrow::<UINavigationItemHostObject>(this).navigation_bar
}
- (bool)hidesBackButton {
    env.objc.borrow::<UINavigationItemHostObject>(this).hides_back_button
}
- (())setHidesBackButton:(bool)hides {
    env.objc.borrow_mut::<UINavigationItemHostObject>(this).hides_back_button = hides;
}
- (bool)leftItemsSupplementBackButton {
    env.objc.borrow::<UINavigationItemHostObject>(this).left_items_supplement_back_button
}
- (())setLeftItemsSupplementBackButton:(bool)supplements {
    env.objc.borrow_mut::<UINavigationItemHostObject>(this).left_items_supplement_back_button = supplements;
}

@end

@implementation UINavigationBar: UIView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UINavigationBarHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())dealloc {
    let UINavigationBarHostObject {
        superclass: _,
        bar_style: _,
        bar_metrics: _,
        translucent: _,
        items,
        pending_item,
        item_buttons,
        title_label,
        title_view,
    } = std::mem::take(env.objc.borrow_mut(this));
    release(env, pending_item);
    release(env, title_label);
    release(env, title_view);
    let _ = items;
    let _ = item_buttons;
    msg_super![env; this dealloc]
}

- (id)initWithFrame:(CGRect)frame {
    let this: id = msg_super![env; this initWithFrame:frame];
    env.objc.borrow_mut::<UINavigationBarHostObject>(this).bar_style = BAR_STYLE_DEFAULT;
    let color: id = msg_class![env; UIColor blackColor];
    () = msg![env; this setBackgroundColor:color];
    this
}

- (id)init {
    msg![env; this initWithFrame:(<CGRect as Default>::default())]
}

// NSCoding implementation
- (id)initWithCoder:(id)coder {
    let this: id = msg_super![env; this initWithCoder:coder];

    let style_key = ns_string::get_static_str(env, "UIBarStyle");
    let mut bar_style: i64 = msg![env; coder decodeInt64ForKey:style_key];

    // `UIView initWithCoder:` does not decode `UIBarStyle`, so a bar from a nib
    // keeps whatever the default was. UIKit's default nib value is
    // `UIBarStyleBlackTranslucent`, which this emulates as black opaque.
    if bar_style == 0 {
        bar_style = BAR_STYLE_BLACK_TRANSLUCENT;
    }

    let items_key = ns_string::get_static_str(env, "UIItems");
    let items: id = msg![env; coder decodeObjectForKey:items_key];

    {
        let host_obj = env.objc.borrow_mut::<UINavigationBarHostObject>(this);
        host_obj.bar_style = bar_style;
        host_obj.bar_metrics = BAR_METRICS_DEFAULT;
        // Only `UIBarStyleBlackTranslucent` is translucent; the other styles
        // are opaque.
        host_obj.translucent = bar_style == BAR_STYLE_BLACK_TRANSLUCENT;
    }

    // NB: the items in this array are NOT pushed onto the stack. UIKit treats
    // `UIItems` as the nib's initial top item, and a bar laid out from a nib
    // has an empty stack. Pushing them would make `items` report one item too
    // many, which matters because a navigation controller pushes the same item
    // again when its root view controller appears.
    let count: NSUInteger = if items == nil { 0 } else { msg![env; items count] };
    if count > 0 {
        let first_index: NSUInteger = 0;
        let first: id = msg![env; items objectAtIndex:first_index];
        retain(env, first);
        env.objc.borrow_mut::<UINavigationBarHostObject>(this).pending_item = first;
        if count > 1 {
            log!(
                "TODO: UINavigationBar initWithCoder: ignoring {} items past the first",
                count - 1
            );
        }
    }

    if bar_style == BAR_STYLE_BLACK_TRANSLUCENT || bar_style == 2 {
        let color: id = msg_class![env; UIColor blackColor];
        () = msg![env; this setBackgroundColor:color];
    }

    env.objc.borrow_mut::<UINavigationBarHostObject>(this).bar_style = bar_style;

    this
}

// Private: called by an item decoding itself, to register with its bar. Kept
// out of the public API because UIKit has no equivalent.
- (())_touchHLE_adoptNavigationItem:(id)item {
    retain(env, item);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationBarHostObject>(this).pending_item, item);
    release(env, old);
    // NB: no `setNeedsLayout` here. This runs in the middle of the bar's own
    // `initWithCoder:`, and laying out now would re-enter the nib decoder
    // while it is still handing out per-object scratch values, which corrupts
    // the decode. The bar lays out when it is first drawn instead.
}

- (i64)barStyle {
    env.objc.borrow::<UINavigationBarHostObject>(this).bar_style
}
- (())setBarStyle:(i64)bar_style {
    env.objc.borrow_mut::<UINavigationBarHostObject>(this).bar_style = bar_style;
}

- (i64)barMetrics {
    env.objc.borrow::<UINavigationBarHostObject>(this).bar_metrics
}

- (bool)isTranslucent {
    env.objc.borrow::<UINavigationBarHostObject>(this).translucent
}
- (())setTranslucent:(bool)translucent {
    env.objc.borrow_mut::<UINavigationBarHostObject>(this).translucent = translucent;
}

- (id)items {
    let items = env.objc.borrow::<UINavigationBarHostObject>(this).items.clone();
    ns_array::from_vec(env, items)
}

- (id)topItem {
    top_item(env, this)
}
- (id)backItem {
    let items = env.objc.borrow::<UINavigationBarHostObject>(this).items.clone();
    let count = items.len();
    if count >= 2 { items[count - 2] } else { nil }
}

- (())pushNavigationItem:(id)item animated:(bool)_animated { // UINavigationItem *
    tear_down_top_item_views(env, this);
    retain(env, item);
    env.objc.borrow_mut::<UINavigationBarHostObject>(this).items.push(item);
    env.objc.borrow_mut::<UINavigationItemHostObject>(item).navigation_bar = this;
}

- (id)popNavigationItemAnimated:(bool)_animated {
    tear_down_top_item_views(env, this);
    let popped = env.objc.borrow_mut::<UINavigationBarHostObject>(this).items.pop();
    if let Some(popped) = popped {
        env.objc.borrow_mut::<UINavigationItemHostObject>(popped).navigation_bar = nil;
        release(env, popped);
    }
    popped.unwrap_or(nil)
}

- (())setItems:(id)new_items { // NSArray<UINavigationItem *>*
    tear_down_top_item_views(env, this);
    let new_items: Vec<id> = if new_items == nil {
        Vec::new()
    } else {
        let count: u32 = msg![env; new_items count];
        (0..count)
            .map(|i: NSUInteger| {
                let item: id = msg![env; new_items objectAtIndex:i];
                retain(env, item);
                env.objc.borrow_mut::<UINavigationItemHostObject>(item).navigation_bar = this;
                item
            })
            .collect()
    };
    {
        let host_obj = env.objc.borrow_mut::<UINavigationBarHostObject>(this);
        for item in std::mem::replace(&mut host_obj.items, new_items) {
            release(env, item);
        }
    }
}

- (())setDelegate:(id)delegate { // UINavigationBarDelegate *
    // TODO: no delegate messages are sent yet.
    let _ = delegate;
}

- (())layoutSubviews {
    // Detach whatever is showing the previous top item. `bounds` is read first
    // because tearing down sends messages and the borrow inside must not span
    // them.
    let bounds: CGRect = msg![env; this bounds];
    tear_down_top_item_views(env, this);

    if bounds.size.width <= 0.0 {
        return;
    }

    // A bar created in code has no frame until it is given one. Size it to the
    // screen width when it is the only thing to go on, so that a bar with a
    // title still shows it.
    if bounds.size.height <= 0.0 {
        let screen: id = msg_class![env; UIScreen mainScreen];
        let screen_bounds: CGRect = msg![env; screen bounds];
        let bar_metrics = env.objc.borrow::<UINavigationBarHostObject>(this).bar_metrics;
        let height = bar_height(bar_metrics);
        let frame: CGRect = msg![env; this frame];
        let new_frame = CGRect {
            origin: frame.origin,
            size: CGSize { width: screen_bounds.size.width, height },
        };
        () = msg![env; this setFrame:new_frame];
        let new_bounds = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize { width: screen_bounds.size.width, height },
        };
        () = msg![env; this setBounds:new_bounds];
    }

    let bounds: CGRect = msg![env; this bounds];
    let item = top_item(env, this);
    if item != nil {
        layout_top_item(env, this, item, bounds);
    }
}

@end

};
