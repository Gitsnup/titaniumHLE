/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CGContext.h`

use super::cg_affine_transform::{CGAffineTransform, CGAffineTransformIdentity};
use super::cg_bitmap_context::{
    CGBitmapContextDrawer, CGBitmapContextGetHeight, CGBitmapContextGetWidth,
};
use super::cg_color::CGColorRef;
use super::cg_color_space::{
    kCGColorSpaceModelMonochrome, kCGColorSpaceModelRGB, CGColorSpaceGetModel, CGColorSpaceModel,
    CGColorSpaceRef,
};
use super::cg_font::{CGFontHostObject, CGFontRef, CGFontRelease, CGFontRetain, CGGlyph};
use super::cg_geometry::CGPointZero;
use super::cg_image::CGImageRef;
use super::{cg_bitmap_context, cg_color, CGFloat, CGPoint, CGRect, CGSize};
use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::core_foundation::{CFRelease, CFRetain, CFTypeRef};
use crate::frameworks::uikit;
use crate::mem::{ConstPtr, GuestUSize};
use crate::objc::{objc_classes, ClassExports, HostObject};
use crate::Environment;

type CGInterpolationQuality = i32;

type CGTextDrawingMode = i32;
const kCGTextFill: CGTextDrawingMode = 0;
const kCGTextFillStroke: CGTextDrawingMode = 2;

pub type CGBlendMode = i32;
pub const kCGBlendModeNormal: CGBlendMode = 0;
pub const kCGBlendModeMultiply: CGBlendMode = 1;
pub const kCGBlendModeScreen: CGBlendMode = 2;
#[allow(unused)]
pub const kCGBlendModeOverlay: CGBlendMode = 3;
pub const kCGBlendModeDarken: CGBlendMode = 4;
pub const kCGBlendModeLighten: CGBlendMode = 5;
pub const kCGBlendModeCopy: CGBlendMode = 17;

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// CGContext seems to be a CFType-based type, but in our implementation those
// are just Objective-C types, so we need a class for it, but its name is not
// visible anywhere.
@implementation _touchHLE_CGContext: NSObject

- (())dealloc {
    let host_obj = env.objc.borrow::<CGContextHostObject>(this);
    let CGContextSubclass::CGBitmapContext(bitmap_data) = host_obj.subclass;
    if bitmap_data.data_is_owned {
        env.mem.free(bitmap_data.data);
    }
    CGFontRelease(env, host_obj.font);

    env.objc.dealloc_object(this, &mut env.mem)
}

@end

};

#[derive(Clone, Debug)]
pub(super) struct CGPathSubpath {
    /// Points in device space.
    pub points: Vec<CGPoint>,
    /// Whether the subpath is explicitly closed (a closing edge is always
    /// implied when filling, regardless of this flag).
    pub closed: bool,
}

// TODO: keep more states saved once they are implemented
type ContextState = (
    (CGFloat, CGFloat, CGFloat, CGFloat), // RGB fill color
    (CGFloat, CGFloat, CGFloat, CGFloat), // RGB stroke color
    CGAffineTransform,                    // transform
    CGFontRef,                            // font
    CGFloat,                              // font size
    CGFloat,                              // line width
    CGBlendMode,                          // blend mode
);

pub(super) struct CGContextHostObject {
    pub(super) subclass: CGContextSubclass,
    pub(super) rgb_fill_color: (CGFloat, CGFloat, CGFloat, CGFloat),
    pub(super) rgb_stroke_color: (CGFloat, CGFloat, CGFloat, CGFloat),
    pub(super) fill_color_space_model: CGColorSpaceModel,
    pub(super) stroke_color_space_model: CGColorSpaceModel,
    pub(super) line_width: CGFloat,
    /// Subpaths of the current path, with points already transformed to
    /// device space (Core Graphics transforms points when they are added to
    /// the path).
    pub(super) path: Vec<CGPathSubpath>,
    /// Current point of the path, in device space.
    pub(super) path_current_point: Option<CGPoint>,
    pub(super) font: CGFontRef,
    pub(super) font_size: CGFloat,
    /// Current transform.
    pub(super) transform: CGAffineTransform,
    pub(super) blend_mode: CGBlendMode,
    /// Text transform.
    pub(super) text_transform: Option<CGAffineTransform>,
    pub(super) state_stack: Vec<ContextState>,
}
impl HostObject for CGContextHostObject {}

pub(super) enum CGContextSubclass {
    CGBitmapContext(cg_bitmap_context::CGBitmapContextData),
}

pub type CGContextRef = CFTypeRef;

pub fn CGContextRelease(env: &mut Environment, c: CGContextRef) {
    if !c.is_null() {
        CFRelease(env, c);
    }
}
pub fn CGContextRetain(env: &mut Environment, c: CGContextRef) -> CGContextRef {
    if !c.is_null() {
        CFRetain(env, c)
    } else {
        c
    }
}

fn CGContextSetBlendMode(env: &mut Environment, context: CGContextRef, blend_mode: CGBlendMode) {
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .blend_mode = blend_mode;
}

fn CGContextSetFillColorSpace(
    env: &mut Environment,
    context: CGContextRef,
    space: CGColorSpaceRef,
) {
    let color_model = CGColorSpaceGetModel(env, space);
    assert!(color_model == kCGColorSpaceModelMonochrome || color_model == kCGColorSpaceModelRGB);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .fill_color_space_model = color_model;
}

fn CGContextSetStrokeColorSpace(
    env: &mut Environment,
    context: CGContextRef,
    space: CGColorSpaceRef,
) {
    let color_model = CGColorSpaceGetModel(env, space);
    assert!(color_model == kCGColorSpaceModelMonochrome || color_model == kCGColorSpaceModelRGB);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .stroke_color_space_model = color_model;
}

fn CGContextSetFillColorWithColor(env: &mut Environment, context: CGContextRef, color: CGColorRef) {
    let (r, g, b, a) = cg_color::to_rgba(&env.objc, color);
    CGContextSetRGBFillColor(env, context, r, g, b, a)
}

pub fn CGContextSetRGBFillColor(
    env: &mut Environment,
    context: CGContextRef,
    red: CGFloat,
    green: CGFloat,
    blue: CGFloat,
    alpha: CGFloat,
) {
    let color = (red, green, blue, alpha);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_fill_color = color;
}

fn CGContextSetGrayFillColor(
    env: &mut Environment,
    context: CGContextRef,
    gray: CGFloat,
    alpha: CGFloat,
) {
    let color = (gray, gray, gray, alpha);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_fill_color = color;
}

fn CGContextSetGrayStrokeColor(
    env: &mut Environment,
    context: CGContextRef,
    gray: CGFloat,
    alpha: CGFloat,
) {
    let color = (gray, gray, gray, alpha);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_stroke_color = color;
}
fn CGContextSetRGBStrokeColor(
    env: &mut Environment,
    context: CGContextRef,
    r: CGFloat,
    g: CGFloat,
    b: CGFloat,
    a: CGFloat,
) {
    let color = (r, g, b, a);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_stroke_color = color;
}

fn CGContextSetShadowWithColor(
    _env: &mut Environment,
    context: CGContextRef,
    offset: CGSize,
    blur: CGFloat,
    color: CGColorRef,
) {
    log!(
        "TODO: CGContextSetShadowWithColor({:?}, {}, {}, {:?})",
        context,
        offset,
        blur,
        color
    );
}

pub fn CGContextFillRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    cg_bitmap_context::fill_rect(env, context, rect, /* clear: */ false);
}

pub fn CGContextClearRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    cg_bitmap_context::fill_rect(env, context, rect, /* clear: */ true);
}

fn CGContextClipToRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    if rect.origin == CGPointZero
        && rect.size.height == CGBitmapContextGetHeight(env, context) as f32
        && rect.size.width == CGBitmapContextGetWidth(env, context) as f32
    {
        assert!(env
            .objc
            .borrow_mut::<CGContextHostObject>(context)
            .transform
            .is_identity());
        // All good, clipping is not needed!
        return;
    }
    todo!();
}

pub fn CGContextConcatCTM(
    env: &mut Environment,
    context: CGContextRef,
    transform: CGAffineTransform,
) {
    log_dbg!("CGContextConcatCTM({:?})", transform);
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    host_obj.transform = transform.concat(host_obj.transform);
}
pub fn CGContextGetCTM(env: &mut Environment, context: CGContextRef) -> CGAffineTransform {
    let res = env.objc.borrow::<CGContextHostObject>(context).transform;
    log_dbg!("CGContextGetCTM() => {:?}", res);
    res
}
pub fn CGContextRotateCTM(env: &mut Environment, context: CGContextRef, angle: CGFloat) {
    log_dbg!("CGContextRotateCTM({:?})", angle);
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    host_obj.transform = host_obj.transform.rotate(angle);
}
pub fn CGContextScaleCTM(env: &mut Environment, context: CGContextRef, x: CGFloat, y: CGFloat) {
    log_dbg!("CGContextScaleCTM({:?})", (x, y));
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    host_obj.transform = host_obj.transform.scale(x, y);
}
pub fn CGContextTranslateCTM(
    env: &mut Environment,
    context: CGContextRef,
    tx: CGFloat,
    ty: CGFloat,
) {
    log_dbg!("CGContextTranslateCTM({:?})", (tx, ty));
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    host_obj.transform = host_obj.transform.translate(tx, ty);
}

pub fn CGContextDrawImage(
    env: &mut Environment,
    context: CGContextRef,
    rect: CGRect,
    image: CGImageRef,
) {
    cg_bitmap_context::draw_image(env, context, rect, image);
}

pub fn CGContextSaveGState(env: &mut Environment, context: CGContextRef) {
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    host_obj.state_stack.push((
        host_obj.rgb_fill_color,
        host_obj.rgb_stroke_color,
        host_obj.transform,
        host_obj.font,
        host_obj.font_size,
        host_obj.line_width,
        host_obj.blend_mode,
    ));
    CGFontRetain(env, env.objc.borrow::<CGContextHostObject>(context).font);
}

pub fn CGContextRestoreGState(env: &mut Environment, context: CGContextRef) {
    // We need to release _old_ font, there are 2 cases:
    // - font hasn't been set between save/restore -> this release corresponds
    // the font retain from save
    // - font has been set between save/restore -> we need to release old font
    // retained on the set
    CGFontRelease(env, env.objc.borrow::<CGContextHostObject>(context).font);
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    let state = host_obj.state_stack.pop().unwrap();
    host_obj.rgb_fill_color = state.0;
    host_obj.rgb_stroke_color = state.1;
    host_obj.transform = state.2;
    host_obj.font = state.3;
    host_obj.font_size = state.4;
    host_obj.line_width = state.5;
    host_obj.blend_mode = state.6;
}

fn CGContextSetInterpolationQuality(
    _env: &mut Environment,
    context: CGContextRef,
    quality: CGInterpolationQuality,
) {
    log!(
        "TODO: CGContextSetInterpolationQuality({:?}, {:?})",
        context,
        quality
    );
}
fn CGContextSetAllowsAntialiasing(_env: &mut Environment, context: CGContextRef, allow: bool) {
    log!(
        "TODO: CGContextSetAllowsAntialiasing({:?}, {})",
        context,
        allow
    );
}

fn CGContextSetShouldSmoothFonts(_env: &mut Environment, context: CGContextRef, should: bool) {
    log!(
        "TODO: CGContextSetShouldSmoothFonts({:?}, {})",
        context,
        should
    );
}

fn CGContextSetFont(env: &mut Environment, context: CGContextRef, font: CGFontRef) {
    CGFontRetain(env, font);
    let old_font = env.objc.borrow_mut::<CGContextHostObject>(context).font;
    CGFontRelease(env, old_font);
    env.objc.borrow_mut::<CGContextHostObject>(context).font = font;
}

fn CGContextSetFontSize(env: &mut Environment, context: CGContextRef, size: CGFloat) {
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .font_size = size;
}

fn CGContextSetTextDrawingMode(
    _env: &mut Environment,
    _context: CGContextRef,
    mode: CGTextDrawingMode,
) {
    assert!(mode == kCGTextFill || mode == kCGTextFillStroke); // TODO: support other modes
}

fn CGContextSetTextMatrix(
    env: &mut Environment,
    context: CGContextRef,
    transform: CGAffineTransform,
) {
    log_dbg!("CGContextSetTextMatrix({:?})", transform);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .text_transform = Some(transform);
}

fn CGContextShowGlyphsAtPoint(
    env: &mut Environment,
    context: CGContextRef,
    x: CGFloat,
    y: CGFloat,
    glyphs: ConstPtr<CGGlyph>,
    count: GuestUSize,
) {
    let mut glyph_ids = Vec::new();
    for i in 0..count {
        let glyph_id = env.mem.read(glyphs + i);
        glyph_ids.push(rusttype::GlyphId(glyph_id));
    }

    let font = env.objc.borrow::<CGContextHostObject>(context).font;
    let font_size = env.objc.borrow::<CGContextHostObject>(context).font_size;
    let text_transform = env
        .objc
        .borrow::<CGContextHostObject>(context)
        .text_transform
        .unwrap_or(CGAffineTransformIdentity);

    let font = &env.objc.borrow::<CGFontHostObject>(font).font;

    let mut drawer = CGBitmapContextDrawer::new(&env.objc, &mut env.mem, context);
    let fill_color = drawer.rgb_fill_color();

    font.draw_glyphs(
        font_size,
        glyph_ids,
        (x, y),
        text_transform,
        |raster_glyph| {
            uikit::ui_font::draw_font_glyph(
                &mut drawer,
                raster_glyph,
                fill_color,
                /* clip_x: */ None,
                /* clip_y: */ None,
            )
        },
    );
}

fn CGContextShowGlyphsAtPositions(
    env: &mut Environment,
    context: CGContextRef,
    glyphs: ConstPtr<CGGlyph>,
    positions: ConstPtr<CGPoint>,
    count: GuestUSize,
) {
    let text_transform = env
        .objc
        .borrow::<CGContextHostObject>(context)
        .text_transform
        .unwrap_or(CGAffineTransformIdentity);
    assert!(text_transform.tx == 0.0 && text_transform.ty == 0.0); // TODO

    for i in 0..count {
        let glyph_ptr = glyphs + i;
        let point = env.mem.read(positions + i);
        let transformed_point = text_transform.apply_to_point(point);
        CGContextShowGlyphsAtPoint(
            env,
            context,
            transformed_point.x,
            transformed_point.y,
            glyph_ptr,
            1,
        );
    }
}

// The `CGContextSetFillColor` and `CGContextSetStrokeColor` functions interpret
// the component array according to the fill/stroke color space. We only
// support the monochrome and RGB models (the only ones we accept in the
// `...ColorSpace` setters anyway).
fn CGContextSetFillColor(
    env: &mut Environment,
    context: CGContextRef,
    components: ConstPtr<CGFloat>,
) {
    let color = match env
        .objc
        .borrow::<CGContextHostObject>(context)
        .fill_color_space_model
    {
        kCGColorSpaceModelMonochrome => {
            let gray = env.mem.read(components);
            let alpha = env.mem.read(components + 1);
            (gray, gray, gray, alpha)
        }
        _ => {
            let r = env.mem.read(components);
            let g = env.mem.read(components + 1);
            let b = env.mem.read(components + 2);
            let a = env.mem.read(components + 3);
            (r, g, b, a)
        }
    };
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_fill_color = color;
}

fn CGContextSetStrokeColor(
    env: &mut Environment,
    context: CGContextRef,
    components: ConstPtr<CGFloat>,
) {
    let color = match env
        .objc
        .borrow::<CGContextHostObject>(context)
        .stroke_color_space_model
    {
        kCGColorSpaceModelMonochrome => {
            let gray = env.mem.read(components);
            let alpha = env.mem.read(components + 1);
            (gray, gray, gray, alpha)
        }
        _ => {
            let r = env.mem.read(components);
            let g = env.mem.read(components + 1);
            let b = env.mem.read(components + 2);
            let a = env.mem.read(components + 3);
            (r, g, b, a)
        }
    };
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_stroke_color = color;
}

fn CGContextSetLineWidth(env: &mut Environment, context: CGContextRef, width: CGFloat) {
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .line_width = width;
}

/// Terminates the currently open subpath, if any.
fn close_current_subpath(host_obj: &mut CGContextHostObject) {
    if let Some(subpath) = host_obj.path.last_mut() {
        subpath.closed = true;
    }
}

/// Adds a device-space point to the current path, starting a new subpath (from
/// the current point) if the previous one is closed or doesn't exist.
fn add_path_point(host_obj: &mut CGContextHostObject, point: CGPoint) {
    let need_new_subpath = host_obj.path.last().map_or(true, |subpath| subpath.closed);
    if need_new_subpath {
        let mut subpath: Vec<CGPoint> = Vec::new();
        if let Some(current) = host_obj.path_current_point {
            subpath.push(current);
        }
        host_obj.path.push(CGPathSubpath {
            points: subpath,
            closed: false,
        });
    }
    host_obj.path.last_mut().unwrap().points.push(point);
    host_obj.path_current_point = Some(point);
}

fn CGContextBeginPath(env: &mut Environment, context: CGContextRef) {
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    host_obj.path.clear();
    host_obj.path_current_point = None;
}

fn CGContextMoveToPoint(env: &mut Environment, context: CGContextRef, x: CGFloat, y: CGFloat) {
    let transform = env.objc.borrow::<CGContextHostObject>(context).transform;
    let point = transform.apply_to_point(CGPoint { x, y });
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    close_current_subpath(host_obj);
    host_obj.path_current_point = Some(point);
}

fn CGContextAddLineToPoint(env: &mut Environment, context: CGContextRef, x: CGFloat, y: CGFloat) {
    let transform = env.objc.borrow::<CGContextHostObject>(context).transform;
    let point = transform.apply_to_point(CGPoint { x, y });
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    add_path_point(host_obj, point);
}

fn CGContextAddRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    let transform = env.objc.borrow::<CGContextHostObject>(context).transform;
    let x = rect.origin.x;
    let y = rect.origin.y;
    let (w, h) = (rect.size.width, rect.size.height);
    let corners = [
        CGPoint { x, y },
        CGPoint { x: x + w, y },
        CGPoint { x: x + w, y: y + h },
        CGPoint { x, y: y + h },
    ];
    let points: Vec<CGPoint> = corners
        .iter()
        .map(|&point| transform.apply_to_point(point))
        .collect();
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    close_current_subpath(host_obj);
    host_obj.path.push(CGPathSubpath {
        points,
        closed: true,
    });
    host_obj.path_current_point = Some(transform.apply_to_point(rect.origin));
}

/// Appends an arc tangent to the lines `(current point -> (x1, y1))` and
/// `((x1, y1) -> (x2, y2))`, with the given radius, to the current path.
/// The arc is approximated with straight line segments.
fn CGContextAddArcToPoint(
    env: &mut Environment,
    context: CGContextRef,
    x1: CGFloat,
    y1: CGFloat,
    x2: CGFloat,
    y2: CGFloat,
    radius: CGFloat,
) {
    let transform = env.objc.borrow::<CGContextHostObject>(context).transform;
    let p1 = CGPoint { x: x1, y: y1 };
    let p2 = CGPoint { x: x2, y: y2 };
    let p0 = env
        .objc
        .borrow::<CGContextHostObject>(context)
        .path_current_point
        .unwrap_or(p1);

    // Unit vectors of the two tangent lines, in user space.
    let (v1x, v1y) = (p0.x - p1.x, p0.y - p1.y);
    let (v2x, v2y) = (p2.x - p1.x, p2.y - p1.y);
    let l1 = (v1x * v1x + v1y * v1y).sqrt();
    let l2 = (v2x * v2x + v2y * v2y).sqrt();
    if l1 == 0.0 || l2 == 0.0 || radius == 0.0 {
        // Degenerate case: just add a line to the tangent point.
        let point = transform.apply_to_point(p1);
        let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
        add_path_point(host_obj, point);
        return;
    }
    let (a1x, a1y) = (v1x / l1, v1y / l1);
    let (a2x, a2y) = (v2x / l2, v2y / l2);

    let cos_angle = (a1x * a2x + a1y * a2y).clamp(-1.0, 1.0);
    let angle = cos_angle.acos();
    // If the lines are parallel there is no arc to be had.
    if angle <= 0.0001 || (std::f32::consts::PI - angle) <= 0.0001 {
        let point = transform.apply_to_point(p1);
        let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
        add_path_point(host_obj, point);
        return;
    }

    let d = radius / (angle / 2.0).tan();
    let t1 = CGPoint {
        x: p1.x + a1x * d,
        y: p1.y + a1y * d,
    };
    let t2 = CGPoint {
        x: p1.x + a2x * d,
        y: p1.y + a2y * d,
    };

    // The center of the arc is radius away from the tangent lines, on the side
    // of the turn.
    let cross = a1x * a2y - a1y * a2x;
    let (nx, ny) = if cross > 0.0 {
        (-a1y, a1x)
    } else {
        (a1y, -a1x)
    };
    let center = CGPoint {
        x: t1.x + nx * radius,
        y: t1.y + ny * radius,
    };

    let a_start = (t1.y - center.y).atan2(t1.x - center.x);
    let a_end = (t2.y - center.y).atan2(t2.x - center.x);
    const TWO_PI: f32 = std::f32::consts::TAU;
    let mut sweep = (a_end - a_start) % TWO_PI;
    if sweep < 0.0 {
        sweep += TWO_PI;
    }
    if cross <= 0.0 {
        sweep -= TWO_PI;
    }
    // ~8 segments per quarter circle.
    let steps = ((sweep.abs() * (16.0 / std::f32::consts::PI)).ceil() as u32).max(2);

    let mut arc_points: Vec<CGPoint> = Vec::with_capacity(steps as usize + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = a_start + sweep * t;
        arc_points.push(CGPoint {
            x: center.x + radius * a.cos(),
            y: center.y + radius * a.sin(),
        });
    }
    // Connect the current point to the start of the arc with a straight line.
    let mut device_points: Vec<CGPoint> = Vec::with_capacity(arc_points.len() + 1);
    device_points.push(transform.apply_to_point(t1));
    for &point in &arc_points {
        device_points.push(transform.apply_to_point(point));
    }

    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    // If the current point is already the start of the arc, don't add it twice.
    let start = if host_obj.path_current_point == Some(device_points[0]) {
        1
    } else {
        0
    };
    for &point in device_points[start..].iter() {
        add_path_point(host_obj, point);
    }
    host_obj.path_current_point = Some(transform.apply_to_point(t2));
}

fn CGContextClosePath(env: &mut Environment, context: CGContextRef) {
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    if let Some(subpath) = host_obj.path.last_mut() {
        if subpath.points.len() >= 2 {
            subpath.closed = true;
            host_obj.path_current_point = Some(subpath.points[0]);
        }
    }
}

/// Rasterizes the current path with the nonzero winding rule, in the fill
/// color. Points must already be in device space.
fn fill_subpaths(drawer: &mut CGBitmapContextDrawer, subpaths: &[CGPathSubpath]) {
    let mut edges: Vec<(CGPoint, CGPoint)> = Vec::new();
    let mut min = CGPoint {
        x: f32::INFINITY,
        y: f32::INFINITY,
    };
    let mut max = CGPoint {
        x: f32::NEG_INFINITY,
        y: f32::NEG_INFINITY,
    };
    for subpath in subpaths {
        let n = subpath.points.len();
        if n < 2 {
            continue;
        }
        for i in 0..n {
            let p1 = subpath.points[i];
            let p2 = subpath.points[(i + 1) % n];
            min.x = min.x.min(p1.x);
            min.y = min.y.min(p1.y);
            max.x = max.x.max(p1.x);
            max.y = max.y.max(p1.y);
            edges.push((p1, p2));
        }
    }
    if edges.is_empty() {
        return;
    }
    let color = drawer.rgb_fill_color();
    let x_start = (min.x.floor().max(0.0) as i32).max(0);
    let y_start = (min.y.floor().max(0.0) as i32).max(0);
    let x_end = ((max.x.ceil() as i32).min(drawer.width() as i32)).max(x_start);
    let y_end = ((max.y.ceil() as i32).min(drawer.height() as i32)).max(y_start);
    for y in y_start..y_end {
        let py = y as f32 + 0.5;
        for x in x_start..x_end {
            let px = x as f32 + 0.5;
            // Nonzero winding rule point-in-polygon test.
            let mut winding = 0;
            for &(p1, p2) in &edges {
                if (p1.y > py) != (p2.y > py) {
                    let x_at = p1.x + (py - p1.y) * (p2.x - p1.x) / (p2.y - p1.y);
                    if x_at > px {
                        winding += if p2.y > p1.y { 1 } else { -1 };
                    }
                }
            }
            if winding != 0 {
                drawer.put_pixel((x, y), color, /* blend: */ true);
            }
        }
    }
}

fn CGContextFillPath(env: &mut Environment, context: CGContextRef) {
    let subpaths: Vec<CGPathSubpath> = env.objc.borrow::<CGContextHostObject>(context).path.clone();
    if subpaths.is_empty() {
        return;
    }
    let mut drawer = CGBitmapContextDrawer::new(&env.objc, &mut env.mem, context);
    fill_subpaths(&mut drawer, &subpaths);
}

/// Draws a single thick line segment by stamping overlapping discs along it.
/// This is crude (no butt caps/joins), but fine for UI strokes.
fn stroke_segment(
    drawer: &mut CGBitmapContextDrawer,
    from: CGPoint,
    to: CGPoint,
    half_width: CGFloat,
) {
    if half_width <= 0.0 {
        return;
    }
    let color = drawer.rgb_stroke_color();
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = (dx * dx + dy * dy).sqrt();
    let steps = (length * 2.0).ceil().max(1.0) as i32;
    let reach = half_width.ceil() as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let (sx, sy) = (from.x + dx * t, from.y + dy * t);
        for oy in -reach..=reach {
            for ox in -reach..=reach {
                if (ox * ox + oy * oy) as f32 <= half_width * half_width {
                    drawer.put_pixel(
                        (sx as i32 + ox, sy as i32 + oy),
                        color,
                        /* blend: */ true,
                    );
                }
            }
        }
    }
}

fn CGContextStrokeLineSegments(
    env: &mut Environment,
    context: CGContextRef,
    points: ConstPtr<CGPoint>,
    count: GuestUSize,
) {
    assert!(count % 2 == 0); // pairs of (start, end) points
    let (transform, line_width) = {
        let host_obj = env.objc.borrow::<CGContextHostObject>(context);
        (host_obj.transform, host_obj.line_width)
    };
    if line_width <= 0.0 {
        return;
    }
    let mut segments: Vec<(CGPoint, CGPoint)> = Vec::with_capacity((count / 2) as usize);
    let mut i = 0;
    while i + 1 < count {
        let from = transform.apply_to_point(env.mem.read(points + i));
        let to = transform.apply_to_point(env.mem.read(points + i + 1));
        segments.push((from, to));
        i += 2;
    }
    let mut drawer = CGBitmapContextDrawer::new(&env.objc, &mut env.mem, context);
    for (from, to) in segments {
        stroke_segment(&mut drawer, from, to, line_width / 2.0);
    }
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CGContextRetain(_)),
    export_c_func!(CGContextRelease(_)),
    export_c_func!(CGContextSetBlendMode(_, _)),
    export_c_func!(CGContextSetFillColorSpace(_, _)),
    export_c_func!(CGContextSetFillColorWithColor(_, _)),
    export_c_func!(CGContextSetRGBFillColor(_, _, _, _, _)),
    export_c_func!(CGContextSetGrayFillColor(_, _, _)),
    export_c_func!(CGContextSetGrayStrokeColor(_, _, _)),
    export_c_func!(CGContextSetRGBStrokeColor(_, _, _, _, _)),
    export_c_func!(CGContextSetStrokeColorSpace(_, _)),
    export_c_func!(CGContextSetFillColor(_, _)),
    export_c_func!(CGContextSetStrokeColor(_, _)),
    export_c_func!(CGContextSetLineWidth(_, _)),
    export_c_func!(CGContextSetShadowWithColor(_, _, _, _)),
    export_c_func!(CGContextFillRect(_, _)),
    export_c_func!(CGContextClearRect(_, _)),
    export_c_func!(CGContextClipToRect(_, _)),
    export_c_func!(CGContextConcatCTM(_, _)),
    export_c_func!(CGContextGetCTM(_)),
    export_c_func!(CGContextRotateCTM(_, _)),
    export_c_func!(CGContextScaleCTM(_, _, _)),
    export_c_func!(CGContextTranslateCTM(_, _, _)),
    export_c_func!(CGContextDrawImage(_, _, _)),
    export_c_func!(CGContextBeginPath(_)),
    export_c_func!(CGContextMoveToPoint(_, _, _)),
    export_c_func!(CGContextAddLineToPoint(_, _, _)),
    export_c_func!(CGContextAddRect(_, _)),
    export_c_func!(CGContextAddArcToPoint(_, _, _, _, _, _)),
    export_c_func!(CGContextClosePath(_)),
    export_c_func!(CGContextFillPath(_)),
    export_c_func!(CGContextStrokeLineSegments(_, _, _)),
    export_c_func!(CGContextSaveGState(_)),
    export_c_func!(CGContextRestoreGState(_)),
    export_c_func!(CGContextSetInterpolationQuality(_, _)),
    export_c_func!(CGContextSetAllowsAntialiasing(_, _)),
    export_c_func!(CGContextSetShouldSmoothFonts(_, _)),
    export_c_func!(CGContextSetFont(_, _)),
    export_c_func!(CGContextSetFontSize(_, _)),
    export_c_func!(CGContextSetTextDrawingMode(_, _)),
    export_c_func!(CGContextSetTextMatrix(_, _)),
    export_c_func!(CGContextShowGlyphsAtPoint(_, _, _, _, _)),
    export_c_func!(CGContextShowGlyphsAtPositions(_, _, _, _)),
];
