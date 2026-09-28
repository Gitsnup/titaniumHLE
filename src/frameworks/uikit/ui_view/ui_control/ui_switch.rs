/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UISwitch`.

use crate::environment::Environment;
use crate::frameworks::core_graphics::cg_bitmap_context::{
    CGBitmapContextCreate, CGBitmapContextCreateImage,
};
use crate::frameworks::core_graphics::cg_color_space::CGColorSpaceCreateDeviceRGB;
use crate::frameworks::core_graphics::cg_context::{
    CGContextFillRect, CGContextRelease, CGContextScaleCTM, CGContextSetRGBFillColor,
    CGContextTranslateCTM,
};
use crate::frameworks::core_graphics::cg_image::kCGImageAlphaPremultipliedLast;
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string;
use crate::frameworks::foundation::ns_string::get_static_str;
use crate::frameworks::uikit::ui_font::UITextAlignmentCenter;
use crate::frameworks::uikit::ui_graphics::{UIGraphicsPopContext, UIGraphicsPushContext};
use crate::frameworks::uikit::ui_view::ui_control::{
    send_actions, UIControlEventTouchUpInside, UIControlEventValueChanged,
};
use crate::mem::Ptr;
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_class, msg_super, nil, objc_classes, release,
    ClassExports, NSZonePtr,
};

// TODO: rendering: shadows, gradients. etc.
// TODO: animation
// TODO: drag-to-flip

// Those parameters are not "exact",
// but just chosen to look nice ;)
const BACK_INSET: f32 = 1.0;
const THUMB_INSET: f32 = 1.0;
const CORNER_RADIUS: f32 = 5.0;
const THUMB_WIDTH: f32 = 42.0;
// Those correspond to debugDescription output of UISwitch
const TOTAL_WIDTH: f32 = 94.0;
const TOTAL_HEIGHT: f32 = 27.0;

pub struct UISwitchHostObject {
    superclass: super::UIControlHostObject,
    is_on: bool,
    /// `UIView*`
    back: id,
    /// `UIView*`
    thumb: id,
    /// `UILabel*`
    label_on: id,
    /// `UILabel*`
    label_off: id,
}
impl_HostObject_with_superclass!(UISwitchHostObject);
impl Default for UISwitchHostObject {
    fn default() -> Self {
        UISwitchHostObject {
            superclass: Default::default(),
            is_on: false,
            thumb: nil,
            back: nil,
            label_on: nil,
            label_off: nil,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SwitchImageKind {
    /// Track with the glossy aqua ON half on the left
    TrackOn,
    /// Track with both halves frosted silver
    TrackOff,
    /// Glossy white thumb orb
    Thumb,
}

/// Frutiger Aero-style glossy artwork for the switch, drawn per pixel row
/// (the CGContext implementation has no gradient primitives).
fn make_switch_image(env: &mut Environment, kind: SwitchImageKind) -> id {
    let (width, height): (u32, f32) = match kind {
        SwitchImageKind::TrackOn | SwitchImageKind::TrackOff => (92, 25.0),
        SwitchImageKind::Thumb => (40, 23.0),
    };
    let (width_f, height_f) = (width as f32, height);

    let color_space = CGColorSpaceCreateDeviceRGB(env);
    let context = CGBitmapContextCreate(
        env,
        Ptr::null(),
        width,
        height as u32,
        8,
        4 * width,
        color_space,
        kCGImageAlphaPremultipliedLast,
    );
    UIGraphicsPushContext(env, context);

    // Compensate for row order inversion (y=0 becomes the top row)
    CGContextTranslateCTM(env, context, 0.0, height_f);
    CGContextScaleCTM(env, context, 1.0, -1.0);

    // Vertical gradient fill helper
    fn fill_gradient(
        env: &mut Environment,
        context: id,
        rect: CGRect,
        top: (f32, f32, f32),
        bottom: (f32, f32, f32),
    ) {
        let rows = rect.size.height as u32;
        for row in 0..rows {
            let t = row as f32 / (rows - 1) as f32;
            CGContextSetRGBFillColor(
                env,
                context,
                (top.0 + (bottom.0 - top.0) * t) as CGFloat,
                (top.1 + (bottom.1 - top.1) * t) as CGFloat,
                (top.2 + (bottom.2 - top.2) * t) as CGFloat,
                1.0,
            );
            CGContextFillRect(
                env,
                context,
                CGRect {
                    origin: CGPoint {
                        x: rect.origin.x,
                        y: rect.origin.y + row as CGFloat,
                    },
                    size: CGSize {
                        width: rect.size.width,
                        height: 1.0,
                    },
                },
            );
        }
    }
    // Top-half glass sheen helper
    fn fill_sheen(env: &mut Environment, context: id, rect: CGRect, strength: f32) {
        let rows = (rect.size.height / 2.0) as u32;
        for row in 0..rows {
            let t = row as f32 / rows as f32;
            let alpha = strength * (1.0 - t) + 0.03;
            CGContextSetRGBFillColor(env, context, 1.0, 1.0, 1.0, alpha as CGFloat);
            CGContextFillRect(
                env,
                context,
                CGRect {
                    origin: CGPoint {
                        x: rect.origin.x,
                        y: rect.origin.y + row as CGFloat,
                    },
                    size: CGSize {
                        width: rect.size.width,
                        height: 1.0,
                    },
                },
            );
        }
    }

    let full = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: width_f,
            height: height_f,
        },
    };
    match kind {
        // Aqua half on the left (where the "ON" label sits), silver on the right
        SwitchImageKind::TrackOn => {
            let half = CGSize {
                width: width_f / 2.0,
                height: height_f,
            };
            fill_gradient(
                env,
                context,
                CGRect {
                    origin: CGPoint { x: 0.0, y: 0.0 },
                    size: half,
                },
                (0.70, 0.95, 1.0),
                (0.03, 0.45, 0.85),
            );
            fill_gradient(
                env,
                context,
                CGRect {
                    origin: CGPoint {
                        x: width_f / 2.0,
                        y: 0.0,
                    },
                    size: half,
                },
                (0.92, 0.94, 0.97),
                (0.55, 0.60, 0.68),
            );
            fill_sheen(env, context, full, 0.5);
        }
        SwitchImageKind::TrackOff => {
            fill_gradient(env, context, full, (0.92, 0.94, 0.97), (0.55, 0.60, 0.68));
            fill_sheen(env, context, full, 0.5);
        }
        SwitchImageKind::Thumb => {
            fill_gradient(env, context, full, (1.0, 1.0, 1.0), (0.78, 0.81, 0.86));
            fill_sheen(env, context, full, 0.75);
        }
    }

    UIGraphicsPopContext(env);

    let cg_image = CGBitmapContextCreateImage(env, context);
    CGContextRelease(env, context);

    let ui_image: id = msg_class![env; UIImage imageWithCGImage:cg_image];
    release(env, cg_image);

    ui_image
}

fn update(env: &mut Environment, this: id) {
    let &mut UISwitchHostObject {
        is_on,
        back,
        label_on,
        label_off,
        ..
    } = env.objc.borrow_mut(this);

    let enabled: bool = msg![env; this isEnabled];
    if enabled {
        () = msg![env; this setAlpha:1.0f32];
    } else {
        () = msg![env; this setAlpha:0.8f32];
    };

    let track_image = make_switch_image(
        env,
        if is_on {
            SwitchImageKind::TrackOn
        } else {
            SwitchImageKind::TrackOff
        },
    );
    () = msg![env; back setImage:track_image];
    release(env, track_image);

    () = msg![env; label_on setHidden:(!is_on)];
    () = msg![env; label_off setHidden:is_on];

    // we need to force re-layout
    () = msg![env; this layoutSubviews];
}

fn init_common(env: &mut Environment, this: id) -> id {
    let size: CGFloat = 17.0;
    let font: id = msg_class![env; UIFont boldSystemFontOfSize:size];

    let white_color: id = msg_class![env; UIColor whiteColor];
    let light_gray_color: id = msg_class![env; UIColor lightGrayColor];

    // This is actually sets the "border" color
    () = msg![env; this setBackgroundColor:light_gray_color];

    let back: id = msg_class![env; UIImageView new];
    let track_image = make_switch_image(env, SwitchImageKind::TrackOff);
    () = msg![env; back setImage:track_image];
    release(env, track_image);

    let thumb: id = msg_class![env; UIImageView new];
    let thumb_image = make_switch_image(env, SwitchImageKind::Thumb);
    () = msg![env; thumb setImage:thumb_image];
    release(env, thumb_image);

    let label_on: id = msg_class![env; UILabel new];
    let clear_color: id = msg_class![env; UIColor clearColor];
    () = msg![env; label_on setBackgroundColor:clear_color];
    () = msg![env; label_on setTextAlignment:UITextAlignmentCenter];
    let text = ns_string::get_static_str(env, "ON");
    () = msg![env; label_on setText:text];
    () = msg![env; label_on setTextColor:white_color];
    () = msg![env; label_on setFont:font];

    let label_off: id = msg_class![env; UILabel new];
    () = msg![env; label_off setBackgroundColor:clear_color];
    () = msg![env; label_off setTextAlignment:UITextAlignmentCenter];
    let text = ns_string::get_static_str(env, "OFF");
    () = msg![env; label_off setText:text];
    let text_color: id = msg_class![env; UIColor darkGrayColor];
    () = msg![env; label_off setTextColor:text_color];
    () = msg![env; label_off setFont:font];

    let host_obj = env.objc.borrow_mut::<UISwitchHostObject>(this);
    host_obj.back = back;
    host_obj.thumb = thumb;
    host_obj.label_on = label_on;
    host_obj.label_off = label_off;

    () = msg![env; this addSubview:back];
    () = msg![env; this addSubview:label_on];
    () = msg![env; this addSubview:label_off];
    () = msg![env; this addSubview:thumb];
    update(env, this);

    let selector = env
        .objc
        .lookup_selector("_touchHLE_flipAction:forEvent:")
        .unwrap();
    () = msg![env; this addTarget:this
                           action:selector
                 forControlEvents:UIControlEventTouchUpInside];

    this
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UISwitch: UIControl

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UISwitchHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithFrame:(CGRect)frame {
    log_dbg!("[(UISwitch*){:?} initWithFrame:{}] (frame.size is ignored)", this, frame);
    // According to the docs,
    // the size components of the frame rectangle are ignored.
    let frame = CGRect {
        origin: frame.origin,
        size: CGSize {
            width: TOTAL_WIDTH,
            height: TOTAL_HEIGHT,
        },
    };
    let this: id = msg_super![env; this initWithFrame:frame];

    init_common(env, this)
}

- (id)initWithCoder:(id)coder {
    let this: id = msg_super![env; this initWithCoder:coder];

    let this = init_common(env, this);

    let key_ns_string = get_static_str(env, "UISwitchEnabled");
    let enabled: bool = msg![env; coder decodeBoolForKey:key_ns_string];

    let key_ns_string = get_static_str(env, "UISwitchOn");
    let is_on: bool = msg![env; coder decodeBoolForKey:key_ns_string];

    () = msg![env; this setEnabled:enabled];
    () = msg![env; this setOn:is_on animated:false];

    this
}

- (())layoutSubviews {
    let &mut UISwitchHostObject {
        is_on,
        back,
        thumb,
        label_on,
        label_off,
        ..
    } = env.objc.borrow_mut(this);

    let bounds: CGRect = msg![env; this bounds];
    let radius = CORNER_RADIUS;

    let back_rect: CGRect = CGRect {
        origin: CGPoint {
            x: bounds.origin.x + BACK_INSET,
            y: bounds.origin.y + BACK_INSET
        },
        size: CGSize {
            width: bounds.size.width - 2.0 * BACK_INSET,
            height: bounds.size.height - 2.0 * BACK_INSET,
        }
    };
    let back_radius = radius - BACK_INSET;
    // Below rects are all defined in reference to back_rect, not whole bounds,
    // in order to accommodate for the border
    let thumb_rect: CGRect = if is_on {
        CGRect {
            origin: CGPoint {
                x: back_rect.size.width - (THUMB_WIDTH - 2.0 * THUMB_INSET),
                y: back_rect.origin.y + THUMB_INSET
            },
            size: CGSize {
                width: THUMB_WIDTH - 2.0 * THUMB_INSET,
                height: back_rect.size.height - 2.0 * THUMB_INSET
            }
        }
    } else {
        CGRect {
            origin: CGPoint {
                x: back_rect.origin.x + THUMB_INSET,
                y: back_rect.origin.y + THUMB_INSET
            },
            size: CGSize {
                width: THUMB_WIDTH - 2.0 * THUMB_INSET,
                height: back_rect.size.height - 2.0 * THUMB_INSET
            }
        }
    };
    let thumb_radius = back_radius - THUMB_INSET;
    let label_on_rect: CGRect = CGRect {
        origin: CGPoint {
            x: back_rect.origin.x,
            y: back_rect.origin.y
        },
        size: CGSize {
            width: back_rect.size.width - THUMB_WIDTH,
            height: back_rect.size.height
        }
    };
    let label_on_radius = back_radius;
    let label_off_rect: CGRect = CGRect {
        origin: CGPoint {
            x: back_rect.origin.x + THUMB_WIDTH,
            y: back_rect.origin.y
        },
        size: CGSize {
            width: back_rect.size.width - THUMB_WIDTH,
            height: back_rect.size.height
        }
    };
    let label_off_radius = back_radius;

    () = msg![env; back setFrame:back_rect];
    () = msg![env; thumb setFrame:thumb_rect];
    () = msg![env; label_on setFrame:label_on_rect];
    () = msg![env; label_off setFrame:label_off_rect];

    fn set_radius(env: &mut Environment, view: id, radius: CGFloat) {
        let layer: id = msg![env; view layer];
        () = msg![env; layer setCornerRadius:radius];
    }
    set_radius(env, this, radius);
    set_radius(env, back, back_radius);
    set_radius(env, label_on, label_on_radius);
    set_radius(env, label_off, label_off_radius);
    set_radius(env, thumb, thumb_radius);
}

- (())dealloc {
    let UISwitchHostObject {
        superclass: _,
        is_on: _,
        back,
        thumb,
        label_on,
        label_off,
    } = std::mem::take(env.objc.borrow_mut(this));

    release(env, back);
    release(env, thumb);
    release(env, label_on);
    release(env, label_off);
    msg_super![env; this dealloc]
}

- (())setEnabled:(bool)enabled {
    () = msg_super![env; this setEnabled:enabled];
    update(env, this);
}
- (())setSelected:(bool)selected {
    () = msg_super![env; this setSelected:selected];
    update(env, this);
}
- (())setHighlighted:(bool)highlighted {
    () = msg_super![env; this setHighlighted:highlighted];
    update(env, this);
}

- (bool)isOn {
     env.objc.borrow::<UISwitchHostObject>(this).is_on
}
- (())setOn:(bool)on {
    msg![env; this setOn:on animated:false]
}
- (())setOn:(bool)on animated:(bool)_animated {
    // TODO: support animation
    env.objc.borrow_mut::<UISwitchHostObject>(this).is_on = on;
    update(env, this);
}

- (id)hitTest:(CGPoint)point
    withEvent:(id)event { // UIEvent* (possibly nil)
    // Hide subviews from hit testing so event goes straight to this control
    if msg![env; this pointInside:point withEvent:event] {
        this
    } else {
        nil
    }
}

// "Private" methods, not a part of API

- (())_touchHLE_flipAction:(id)sender forEvent:(id)event { // UIEvent*
    assert_eq!(this, sender);

    let is_on = env.objc.borrow::<UISwitchHostObject>(this).is_on;
    () = msg![env; this setOn:(!is_on) animated:false];

    send_actions(env, this, event, UIControlEventValueChanged);
}

@end

};
