/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Guest-facing OpenGL ES 2.0 functions.
//!
//! These functions forward to the host's desktop OpenGL implementation (see
//! [crate::gles::gles2_on_gl2]). Where desktop GL differs from OpenGL ES 2.0,
//! this module is responsible for papering over the differences:
//!
//! - FBO functions use the `EXT_framebuffer_object` entry points available in
//!   OpenGL 2.1.
//! - Guest GLSL ES 1.00 shaders are preprocessed before being passed to
//!   `glShaderSource` ([translate_shader_source]).
//! - ES 2.0-specific functions with no desktop equivalent are emulated
//!   (`glGetShaderPrecisionFormat`, `glReleaseShaderCompiler`).
//! - Pointer/offset parameters are disambiguated based on the current buffer
//!   bindings, like in the OpenGL ES 1.1 wrapper.

use crate::dyld::{export_c_func, FunctionExports};
use crate::gles::gles11_raw as gles11; // constants & types
use crate::gles::gles11_raw::types::*;
use crate::gles::gles2_on_gl2::GLES2Active;
use crate::mem::{ConstPtr, ConstVoidPtr, GuestISize, GuestUSize, Mem, MutPtr, MutVoidPtr};
use crate::Environment;
use std::ffi::CStr;
use std::ptr;
use touchHLE_gl_bindings::gl21compat as gl21;

type GuestGLsizeiptr = GuestISize;
type GuestGLintptr = GuestISize;
type HostGLsizeiptr = isize;
type HostGLintptr = isize;

/// pname for `glGetIntegerv`: the currently-bound program object (OpenGL 2.0+).
/// OpenGL ES 2.0 constants that don't exist in the OpenGL ES 1.1 registry.
const GL_SHADING_LANGUAGE_VERSION: GLenum = 0x8b8c;
const GL_LOW_FLOAT: GLenum = 0x8df0;
const GL_MEDIUM_FLOAT: GLenum = 0x8df1;
const GL_HIGH_FLOAT: GLenum = 0x8df2;

/// Sync the current context (which must be a GLES2 context) and performs a
/// function `f` within it. In case of a missing EAGL context for the current
/// thread, returns a default value.
fn with_gles2_and_mem<T, U: Default>(env: &mut Environment, f: T) -> U
where
    T: FnOnce(&mut GLES2Active, &mut Mem) -> U,
{
    if env
        .framework_state
        .opengles
        .current_ctx_for_thread(env.current_thread)
        .is_none()
    {
        log!(
            "Warning: No EAGLContext for thread {}! Ignoring OpenGL ES 2.0 call, returning default value.",
            env.current_thread
        );
        return U::default();
    }

    let mut gles2 = sync_gles2_context(
        &mut env.framework_state.opengles,
        &mut env.objc,
        env.window
            .as_mut()
            .expect("OpenGL ES is not supported in headless mode"),
        env.current_thread,
    );

    f(&mut gles2, &mut env.mem)
}

/// Like [with_gles2_and_mem], but panics on a missing context (for functions
/// that can't return a default value).
fn with_gles2_and_mem_no_skip<T, U>(env: &mut Environment, f: T) -> U
where
    T: FnOnce(&mut GLES2Active, &mut Mem) -> U,
{
    let mut gles2 = sync_gles2_context(
        &mut env.framework_state.opengles,
        &mut env.objc,
        env.window
            .as_mut()
            .expect("OpenGL ES is not supported in headless mode"),
        env.current_thread,
    );

    f(&mut gles2, &mut env.mem)
}

fn sync_gles2_context<'objc, 'win: 'objc>(
    state: &mut super::State,
    objc: &'objc mut crate::objc::ObjC,
    window: &'win mut crate::window::Window,
    current_thread: crate::ThreadId,
) -> Box<GLES2Active<'objc>> {
    let gles_ctx = super::get_thread_context(state, objc, current_thread);
    let gles2_ctx = gles_ctx
        .as_gles2_on_gl2_mut()
        .expect("GLES 2.0 call made while an OpenGL ES 1.1 context is current");
    let active = gles2_ctx.make_current_gles2(window);
    Box::new(active)
}

/// Translate a pointer-or-offset parameter to a host pointer, like the
/// OpenGL ES 1.1 wrapper does. If a buffer is bound to `which_binding`, the
/// guest value is an offset into that buffer and should be passed through
/// unchanged; otherwise it is a guest pointer into app memory.
unsafe fn translate_pointer_or_offset_to_host(
    _gles: &mut GLES2Active,
    mem: &Mem,
    pointer_or_offset: ConstVoidPtr,
    which_binding: GLenum,
) -> *const GLvoid {
    let mut buffer_binding: GLint = 0;
    gl21::GetIntegerv(which_binding, &mut buffer_binding as *mut GLint as *mut _);
    if buffer_binding != 0 {
        let offset = pointer_or_offset.to_bits();
        offset as usize as *const GLvoid
    } else if pointer_or_offset.is_null() {
        ptr::null()
    } else {
        mem.unchecked_ptr_at(pointer_or_offset.cast::<u8>(), 0)
            .cast::<GLvoid>()
    }
}

/// Get a host pointer for a data pointer, translated from guest memory.
/// `size` is an upper bound on the bytes the GL implementation might read
/// at call time (i.e. excluding deferred reads like draw calls).
unsafe fn translate_data_pointer<T>(mem: &Mem, pointer: ConstPtr<T>, size: usize) -> *const T {
    if pointer.is_null() {
        ptr::null()
    } else {
        mem.ptr_at(pointer.cast::<u8>(), size as GuestUSize)
            .cast::<T>()
    }
}

/// Estimate the size (in bytes) of a texture image upload, for bounds
/// checking. Derived from the table in the OpenGL ES 2.0 spec.
fn image_size_estimate(width: GLsizei, height: GLsizei, format: GLenum, type_: GLenum) -> usize {
    let pixel_count = width.unsigned_abs() as usize * height.unsigned_abs() as usize;
    let bytes_per_pixel: usize = match (format, type_) {
        (gles11::ALPHA, gles11::UNSIGNED_BYTE) | (gles11::LUMINANCE, gles11::UNSIGNED_BYTE) => 1,
        (gles11::LUMINANCE_ALPHA, gles11::UNSIGNED_BYTE) => 2,
        (gles11::RGB, gles11::UNSIGNED_BYTE) => 3,
        (gles11::RGB, gles11::UNSIGNED_SHORT_5_6_5) => 2,
        (gles11::RGBA, gles11::UNSIGNED_BYTE) => 4,
        (gles11::RGBA, gles11::UNSIGNED_SHORT_4_4_4_4) => 2,
        (gles11::RGBA, gles11::UNSIGNED_SHORT_5_5_5_1) => 2,
        _ => 4,
    };
    pixel_count * bytes_per_pixel
}

/// Adapt a guest GLSL ES 1.00 shader for the host's desktop GLSL compiler.
///
fn translate_shader_source(source: &str) -> String {
    let mut out = String::new();

    // GLSL ES 1.00 shaders have no #version directive, and if they do have
    // one, it's an ES version that desktop GLSL doesn't accept. GLSL 1.30
    // accepts precision qualifiers, which ES shaders rely on.
    if !source.contains("#version") {
        out.push_str("#version 130\n");
    }

    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#version") {
            // Replace any ES version with a desktop version.
            out.push_str("#version 130\n");
            continue;
        }
        if trimmed.starts_with("#extension") {
            // ES-specific extensions either aren't available (and are
            // optional in ES), or are core functionality in GLSL 1.30 (e.g.
            // GL_OES_standard_derivatives). OpenGL ES 2.0 requires that
            // unsupported extensions cause an error only when the behavior
            // is `enable`/`require`, but the GLSL compiler on the host will
            // error out on unknown extensions regardless, so we drop these
            // lines entirely.
            if trimmed.contains("GL_OES_") || trimmed.contains("GL_EXT_") {
                log_dbg!("Dropping ES shader extension directive: {line:?}");
                continue;
            }
        }
        if trimmed.contains("samplerExternalOES") {
            // GL_OES_EGL_image_external is not available on desktop GL.
            // This is a bit of a hack, but it's the best we can do.
            log_dbg!("Replacing samplerExternalOES with sampler2D in shader");
            out.push_str(&line.replace("samplerExternalOES", "sampler2D"));
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    out
}

// Generic state manipulation

fn glGetError(env: &mut Environment) -> GLenum {
    let ignore_gl_errors = env.options.ignore_gl_errors;
    with_gles2_and_mem(env, |_gles, _mem| {
        let err = unsafe { gl21::GetError() };
        if err != 0 {
            if ignore_gl_errors {
                return 0;
            }
            log!("Warning: glGetError() returned {:#x}", err);
        }
        err
    })
}

fn glEnable(env: &mut Environment, cap: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::Enable(cap) });
}
fn glDisable(env: &mut Environment, cap: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::Disable(cap) });
}
fn glIsEnabled(env: &mut Environment, cap: GLenum) -> GLboolean {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::IsEnabled(cap) })
}
fn glGetIntegerv(env: &mut Environment, pname: GLenum, params: MutPtr<GLint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        match pname {
            gles11::NUM_COMPRESSED_TEXTURE_FORMATS => {
                mem.write(
                    params,
                    super::gles_guest::SUPPORTED_COMPRESSED_TEXTURE_FORMATS.len() as _,
                );
            }
            gles11::COMPRESSED_TEXTURE_FORMATS => {
                for (idx, &format) in super::gles_guest::SUPPORTED_COMPRESSED_TEXTURE_FORMATS
                    .iter()
                    .enumerate()
                {
                    mem.write(params + idx as GuestUSize, format as _);
                }
            }
            _ => {
                let params = mem.ptr_at_mut(params, 16 /* upper bound */);
                gl21::GetIntegerv(pname, params.cast());
            }
        }
    });
}
fn glGetFloatv(env: &mut Environment, pname: GLenum, params: MutPtr<GLfloat>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at_mut(params, 16 /* upper bound */);
        gl21::GetFloatv(pname, params.cast());
    });
}
fn glGetString(env: &mut Environment, name: GLenum) -> ConstPtr<GLubyte> {
    if let Some(&str_) = env.framework_state.opengles.strings_cache.get(&name) {
        return str_;
    }
    // Strings that identify the implementation itself are replaced, so guest
    // apps aren't confused by desktop GL version/extension strings.
    static GLES2_VENDOR: &[u8] = b"touchHLE\0";
    static GLES2_VERSION: &[u8] = b"OpenGL ES 2.0 touchHLE GLES2-on-GL2\0";
    static GLES2_GLSL_VERSION: &[u8] = b"OpenGL ES GLSL ES 1.00 (touchHLE GLES2-on-GL2)\0";
    static GLES2_EXTENSIONS: &[u8] = b"GL_OES_standard_derivatives GL_OES_texture_npot GL_OES_rgb8_rgba8 GL_OES_depth24 GL_OES_depth32 GL_OES_packed_depth_stencil GL_OES_element_index_uint GL_EXT_texture_filter_anisotropic\0";
    let known: Option<&[u8]> = match name {
        gles11::VENDOR | gles11::RENDERER => Some(GLES2_VENDOR),
        gles11::VERSION => Some(GLES2_VERSION),
        GL_SHADING_LANGUAGE_VERSION => Some(GLES2_GLSL_VERSION),
        gles11::EXTENSIONS => Some(GLES2_EXTENSIONS),
        _ => None,
    };
    let res = if let Some(s) = known {
        with_gles2_and_mem_no_skip(env, |_gles, mem| mem.alloc_and_write_cstr(s).cast_const())
    } else {
        with_gles2_and_mem_no_skip(env, |_gles, mem| unsafe {
            let host_str = gl21::GetString(name);
            let s = CStr::from_ptr(host_str as *const std::os::raw::c_char);
            mem.alloc_and_write_cstr(s.to_bytes_with_nul()).cast_const()
        })
    };
    env.framework_state.opengles.strings_cache.insert(name, res);
    res
}
fn glHint(env: &mut Environment, target: GLenum, mode: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::Hint(target, mode) });
}
fn glFinish(env: &mut Environment) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::Finish() });
}
fn glFlush(env: &mut Environment) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::Flush() });
}

// Other state manipulation

fn glBlendFunc(env: &mut Environment, sfactor: GLenum, dfactor: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::BlendFunc(sfactor, dfactor)
    });
}
fn glBlendColor(
    env: &mut Environment,
    red: GLclampf,
    green: GLclampf,
    blue: GLclampf,
    alpha: GLclampf,
) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::BlendColor(red, green, blue, alpha)
    });
}
fn glBlendEquation(env: &mut Environment, mode: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::BlendEquation(mode) });
}
fn glBlendFuncSeparate(
    env: &mut Environment,
    src_rgb: GLenum,
    dst_rgb: GLenum,
    src_alpha: GLenum,
    dst_alpha: GLenum,
) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::BlendFuncSeparate(src_rgb, dst_rgb, src_alpha, dst_alpha)
    });
}
fn glColorMask(
    env: &mut Environment,
    red: GLboolean,
    green: GLboolean,
    blue: GLboolean,
    alpha: GLboolean,
) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::ColorMask(red, green, blue, alpha)
    });
}
fn glCullFace(env: &mut Environment, mode: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::CullFace(mode) });
}
fn glDepthFunc(env: &mut Environment, func: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::DepthFunc(func) });
}
fn glDepthMask(env: &mut Environment, flag: GLboolean) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::DepthMask(flag) });
}
fn glDepthRangef(env: &mut Environment, near: GLclampf, far: GLclampf) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::DepthRange(near as f64, far as f64)
    });
}
fn glFrontFace(env: &mut Environment, mode: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::FrontFace(mode) });
}
fn glLineWidth(env: &mut Environment, width: GLfloat) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::LineWidth(width) });
}
fn glPolygonOffset(env: &mut Environment, factor: GLfloat, units: GLfloat) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::PolygonOffset(factor, units)
    });
}
fn glSampleCoverage(env: &mut Environment, value: GLclampf, invert: GLboolean) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::SampleCoverage(value, invert)
    });
}
fn glScissor(env: &mut Environment, x: GLint, y: GLint, width: GLsizei, height: GLsizei) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::Scissor(x, y, width, height)
    });
}
fn glStencilFunc(env: &mut Environment, func: GLenum, ref_: GLint, mask: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::StencilFunc(func, ref_, mask)
    });
}
fn glStencilFuncSeparate(
    env: &mut Environment,
    face: GLenum,
    func: GLenum,
    ref_: GLint,
    mask: GLuint,
) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::StencilFuncSeparate(face, func, ref_, mask)
    });
}
fn glStencilMask(env: &mut Environment, mask: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::StencilMask(mask) });
}
fn glStencilMaskSeparate(env: &mut Environment, face: GLenum, mask: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::StencilMaskSeparate(face, mask)
    });
}
fn glStencilOp(env: &mut Environment, fail: GLenum, zfail: GLenum, zpass: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::StencilOp(fail, zfail, zpass)
    });
}
fn glStencilOpSeparate(
    env: &mut Environment,
    face: GLenum,
    fail: GLenum,
    zfail: GLenum,
    zpass: GLenum,
) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::StencilOpSeparate(face, fail, zfail, zpass)
    });
}
fn glViewport(env: &mut Environment, x: GLint, y: GLint, width: GLsizei, height: GLsizei) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::Viewport(x, y, width, height)
    });
}

// Buffers

fn glGenBuffers(env: &mut Environment, n: GLsizei, buffers: MutPtr<GLuint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let buffers = mem.ptr_at_mut(buffers, n.unsigned_abs() as _);
        gl21::GenBuffers(n, buffers.cast());
    });
}
fn glDeleteBuffers(env: &mut Environment, n: GLsizei, buffers: ConstPtr<GLuint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let buffers = mem.ptr_at(buffers, n.unsigned_abs() as _);
        gl21::DeleteBuffers(n, buffers.cast());
    });
}
fn glBindBuffer(env: &mut Environment, target: GLenum, buffer: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::BindBuffer(target, buffer)
    });
}
fn glBufferData(
    env: &mut Environment,
    target: GLenum,
    size: GuestGLsizeiptr,
    data: ConstVoidPtr,
    usage: GLenum,
) {
    if size < 0 {
        log_once!("glBufferData called with negative size {size}, ignoring");
        return;
    }
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let data = if data.is_null() {
            ptr::null()
        } else {
            mem.ptr_at(data.cast::<u8>(), size.try_into().unwrap())
                .cast()
        };
        gl21::BufferData(target, size as HostGLsizeiptr, data, usage)
    });
}
fn glBufferSubData(
    env: &mut Environment,
    target: GLenum,
    offset: GuestGLintptr,
    size: GuestGLsizeiptr,
    data: ConstVoidPtr,
) {
    if size < 0 {
        log_once!("glBufferSubData called with negative size {size}, ignoring");
        return;
    }
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let data = if data.is_null() {
            ptr::null()
        } else {
            mem.ptr_at(data.cast::<u8>(), size.try_into().unwrap())
                .cast()
        };
        gl21::BufferSubData(target, offset as HostGLintptr, size as HostGLsizeiptr, data)
    });
}
fn glGetBufferParameteriv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at_mut(params, 1);
        gl21::GetBufferParameteriv(target, pname, params.cast());
    });
}

// Textures

fn glGenTextures(env: &mut Environment, n: GLsizei, textures: MutPtr<GLuint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let textures = mem.ptr_at_mut(textures, n.unsigned_abs() as _);
        gl21::GenTextures(n, textures.cast());
    });
}
fn glDeleteTextures(env: &mut Environment, n: GLsizei, textures: ConstPtr<GLuint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let textures = mem.ptr_at(textures, n.unsigned_abs() as _);
        gl21::DeleteTextures(n, textures.cast());
    });
}
fn glActiveTexture(env: &mut Environment, texture: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::ActiveTexture(texture) });
}
fn glBindTexture(env: &mut Environment, target: GLenum, texture: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::BindTexture(target, texture)
    });
}
fn glIsTexture(env: &mut Environment, texture: GLuint) -> GLboolean {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::IsTexture(texture) })
}
fn glTexParameteri(env: &mut Environment, target: GLenum, pname: GLenum, param: GLint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::TexParameteri(target, pname, param)
    });
}
fn glTexParameterf(env: &mut Environment, target: GLenum, pname: GLenum, param: GLfloat) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::TexParameterf(target, pname, param)
    });
}
fn glTexParameteriv(env: &mut Environment, target: GLenum, pname: GLenum, params: ConstPtr<GLint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at(params, 1);
        gl21::TexParameteriv(target, pname, params.cast());
    });
}
fn glTexParameterfv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: ConstPtr<GLfloat>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at(params, 1);
        gl21::TexParameterfv(target, pname, params.cast());
    });
}
fn glTexImage2D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    internalformat: GLint,
    width: GLsizei,
    height: GLsizei,
    border: GLint,
    format: GLenum,
    type_: GLenum,
    pixels: ConstVoidPtr,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let pixels = if pixels.is_null() {
            ptr::null()
        } else {
            let size = image_size_estimate(width, height, format, type_);
            let host = translate_data_pointer(mem, pixels.cast::<u8>(), size).cast::<u8>();
            host.cast()
        };
        gl21::TexImage2D(
            target,
            level,
            internalformat,
            width,
            height,
            border,
            format,
            type_,
            pixels,
        )
    });
}
fn glTexSubImage2D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    xoffset: GLint,
    yoffset: GLint,
    width: GLsizei,
    height: GLsizei,
    format: GLenum,
    type_: GLenum,
    pixels: ConstVoidPtr,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let pixels = if pixels.is_null() {
            ptr::null()
        } else {
            let size = image_size_estimate(width, height, format, type_);
            translate_data_pointer(mem, pixels.cast::<u8>(), size).cast()
        };
        gl21::TexSubImage2D(
            target, level, xoffset, yoffset, width, height, format, type_, pixels,
        )
    });
}
fn glCompressedTexImage2D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
    border: GLint,
    image_size: GLsizei,
    data: ConstVoidPtr,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let data = if data.is_null() {
            ptr::null()
        } else {
            translate_data_pointer(mem, data.cast::<u8>(), image_size.unsigned_abs() as _).cast()
        };
        gl21::CompressedTexImage2D(
            target,
            level,
            internalformat,
            width,
            height,
            border,
            image_size,
            data,
        )
    });
}
fn glPixelStorei(env: &mut Environment, pname: GLenum, param: GLint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::PixelStorei(pname, param)
    });
}
fn glReadPixels(
    env: &mut Environment,
    x: GLint,
    y: GLint,
    width: GLsizei,
    height: GLsizei,
    format: GLenum,
    type_: GLenum,
    pixels: MutVoidPtr,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let size = image_size_estimate(width, height, format, type_);
        let pixels = if pixels.is_null() {
            ptr::null_mut()
        } else {
            mem.ptr_at_mut(pixels.cast::<u8>(), size as GuestUSize)
                .cast()
        };
        gl21::ReadPixels(x, y, width, height, format, type_, pixels)
    });
}

// Shaders and programs

fn glCreateShader(env: &mut Environment, type_: GLenum) -> GLuint {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::CreateShader(type_) })
}
fn glDeleteShader(env: &mut Environment, shader: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::DeleteShader(shader) });
}
fn glIsShader(env: &mut Environment, shader: GLuint) -> GLboolean {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::IsShader(shader) })
}
fn glShaderSource(
    env: &mut Environment,
    shader: GLuint,
    count: GLsizei,
    string: ConstPtr<ConstPtr<u8>>,
    length: ConstPtr<GLint>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        if count < 0 || string.is_null() {
            return;
        }
        let mut concatenated = String::new();
        let strings = mem.ptr_at(string, count.unsigned_abs() as _);
        for i in 0..count.unsigned_abs() {
            let guest_str = *strings.add(i as usize);
            if guest_str.is_null() {
                continue;
            }
            let str_len: usize = if length.is_null() {
                // Assume NUL-terminated.
                CStr::from_ptr(mem.unchecked_ptr_at(guest_str.cast::<u8>(), 0).cast())
                    .to_bytes()
                    .len()
            } else {
                let lengths = mem.ptr_at(length, count.unsigned_abs() as _);
                let len = *lengths.add(i as usize);
                if len < 0 {
                    CStr::from_ptr(mem.unchecked_ptr_at(guest_str.cast::<u8>(), 0).cast())
                        .to_bytes()
                        .len()
                } else {
                    len as usize
                }
            };
            let bytes = mem.ptr_at(guest_str.cast::<u8>(), str_len as GuestUSize);
            concatenated.push_str(
                std::str::from_utf8(std::slice::from_raw_parts(bytes, str_len)).unwrap_or_default(),
            );
            concatenated.push('\n');
        }
        let translated = translate_shader_source(&concatenated);
        log_dbg!("Shader source for shader {shader:#x} (translated):\n{translated}");
        let source_cstring = std::ffi::CString::new(translated).unwrap();
        let source_ptr: *const gl21::types::GLchar = source_cstring.as_ptr() as _;
        gl21::ShaderSource(
            shader,
            1,
            &source_ptr as *const _ as *const *const gl21::types::GLchar,
            ptr::null(),
        );
    });
}
fn glCompileShader(env: &mut Environment, shader: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::CompileShader(shader);
        let mut status: GLint = 0;
        gl21::GetShaderiv(shader, gl21::COMPILE_STATUS, &mut status);
        if status == 0 {
            let mut len: GLint = 0;
            gl21::GetShaderiv(shader, gl21::INFO_LOG_LENGTH, &mut len);
            let mut buf = vec![0u8; len.max(1) as usize];
            let mut out_len: GLint = 0;
            gl21::GetShaderInfoLog(
                shader,
                len,
                &mut out_len as *mut GLint as *mut _,
                buf.as_mut_ptr() as *mut gl21::types::GLchar,
            );
            log!(
                "glCompileShader({shader:#x}) FAILED: {}",
                String::from_utf8_lossy(&buf[..out_len.max(0) as usize])
            );
        }
    });
}
fn glGetShaderiv(env: &mut Environment, shader: GLuint, pname: GLenum, params: MutPtr<GLint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at_mut(params, 1);
        gl21::GetShaderiv(shader, pname, params.cast());
    });
}
fn glGetShaderInfoLog(
    env: &mut Environment,
    shader: GLuint,
    buf_size: GLsizei,
    length: MutPtr<GLsizei>,
    info_log: MutPtr<u8>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let mut host_len: GLint = 0;
        let host_log_size = buf_size.unsigned_abs().max(1) as HostGLsizeiptr;
        let mut host_buf = vec![0u8; host_log_size as usize];
        gl21::GetShaderInfoLog(
            shader,
            host_log_size as _,
            &mut host_len as *mut GLint as *mut _,
            host_buf.as_mut_ptr() as *mut gl21::types::GLchar,
        );
        if !length.is_null() {
            mem.write(length, host_len.min(buf_size.max(0)) as _);
        }
        if !info_log.is_null() && buf_size > 0 {
            let written = host_len.unsigned_abs().min(buf_size.unsigned_abs() - 1);
            let dest = mem.ptr_at_mut(info_log, written + 1);
            std::ptr::copy_nonoverlapping(host_buf.as_ptr(), dest, written as usize);
            *dest.add(written as usize) = 0;
        }
    });
}
fn glGetShaderSource(
    env: &mut Environment,
    shader: GLuint,
    buf_size: GLsizei,
    length: MutPtr<GLsizei>,
    source: MutPtr<u8>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let mut host_len: GLint = 0;
        gl21::GetShaderSource(
            shader,
            0,
            &mut host_len as *mut GLint as *mut _,
            ptr::null_mut(),
        );
        if !length.is_null() {
            mem.write(length, 0);
        }
        if !source.is_null() && buf_size > 0 {
            *mem.ptr_at_mut(source, 1) = 0;
        }
        // TODO: implement properly if some app relies on it.
        let _ = host_len;
    });
}
fn glReleaseShaderCompiler(env: &mut Environment) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::GetError() });
}
fn glGetShaderPrecisionFormat(
    env: &mut Environment,
    shader_type: GLenum,
    precision_type: GLenum,
    range: MutPtr<GLint>,
    precision: MutPtr<GLint>,
) {
    with_gles2_and_mem(env, |_gles, mem| {
        // Pretend we have full highp precision, which is what desktop GL
        // gives us anyway.
        match precision_type {
            GL_LOW_FLOAT | GL_MEDIUM_FLOAT | GL_HIGH_FLOAT => {
                mem.write(range, 127);
                mem.write(range + 1, 127);
                mem.write(precision, 23);
            }
            _ => {
                mem.write(range, 31);
                mem.write(range + 1, 30);
                mem.write(precision, 0);
            }
        }
        let _ = shader_type;
    });
}
fn glCreateProgram(env: &mut Environment) -> GLuint {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::CreateProgram() })
}
fn glDeleteProgram(env: &mut Environment, program: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::DeleteProgram(program) });
}
fn glIsProgram(env: &mut Environment, program: GLuint) -> GLboolean {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::IsProgram(program) })
}
fn glAttachShader(env: &mut Environment, program: GLuint, shader: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::AttachShader(program, shader)
    });
}
fn glDetachShader(env: &mut Environment, program: GLuint, shader: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::DetachShader(program, shader)
    });
}
fn glLinkProgram(env: &mut Environment, program: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::LinkProgram(program);
        let mut status: GLint = 0;
        gl21::GetProgramiv(program, gl21::LINK_STATUS, &mut status);
        if status == 0 {
            let mut len: GLint = 0;
            gl21::GetProgramiv(program, gl21::INFO_LOG_LENGTH, &mut len);
            let mut buf = vec![0u8; len.max(1) as usize];
            let mut out_len: GLint = 0;
            gl21::GetProgramInfoLog(
                program,
                len,
                &mut out_len as *mut GLint as *mut _,
                buf.as_mut_ptr() as *mut gl21::types::GLchar,
            );
            log!(
                "glLinkProgram({program:#x}) FAILED: {}",
                String::from_utf8_lossy(&buf[..out_len.max(0) as usize])
            );
        }
    });
}
fn glValidateProgram(env: &mut Environment, program: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::ValidateProgram(program) });
}
fn glGetProgramiv(env: &mut Environment, program: GLuint, pname: GLenum, params: MutPtr<GLint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at_mut(params, 1);
        gl21::GetProgramiv(program, pname, params.cast());
    });
}
fn glGetProgramInfoLog(
    env: &mut Environment,
    program: GLuint,
    buf_size: GLsizei,
    length: MutPtr<GLsizei>,
    info_log: MutPtr<u8>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let mut host_len: GLint = 0;
        let host_log_size = buf_size.unsigned_abs().max(1) as HostGLsizeiptr;
        let mut host_buf = vec![0u8; host_log_size as usize];
        gl21::GetProgramInfoLog(
            program,
            host_log_size as _,
            &mut host_len as *mut GLint as *mut _,
            host_buf.as_mut_ptr() as *mut gl21::types::GLchar,
        );
        if !length.is_null() {
            mem.write(length, host_len.min(buf_size.max(0)) as _);
        }
        if !info_log.is_null() && buf_size > 0 {
            let written = host_len.unsigned_abs().min(buf_size.unsigned_abs() - 1);
            let dest = mem.ptr_at_mut(info_log, written + 1);
            std::ptr::copy_nonoverlapping(host_buf.as_ptr(), dest, written as usize);
            *dest.add(written as usize) = 0;
        }
    });
}
fn glGetAttachedShaders(
    env: &mut Environment,
    program: GLuint,
    max_count: GLsizei,
    count: MutPtr<GLsizei>,
    shaders: MutPtr<GLuint>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let count = mem.ptr_at_mut(count, 1);
        let shaders = mem.ptr_at_mut(shaders, max_count.unsigned_abs() as _);
        gl21::GetAttachedShaders(program, max_count, count.cast(), shaders.cast());
    });
}

// Uniforms and attributes

fn glGetUniformLocation(env: &mut Environment, program: GLuint, name: ConstPtr<u8>) -> GLint {
    with_gles2_and_mem_no_skip(env, |_gles, mem| unsafe {
        let name = mem
            .ptr_at(name.cast::<u8>(), 64) // upper bound
            .cast();
        gl21::GetUniformLocation(program, name)
    })
}
fn glGetAttribLocation(env: &mut Environment, program: GLuint, name: ConstPtr<u8>) -> GLint {
    with_gles2_and_mem_no_skip(env, |_gles, mem| unsafe {
        let name = mem
            .ptr_at(name.cast::<u8>(), 64) // upper bound
            .cast();
        gl21::GetAttribLocation(program, name)
    })
}
fn glBindAttribLocation(env: &mut Environment, program: GLuint, index: GLuint, name: ConstPtr<u8>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let name = mem
            .ptr_at(name.cast::<u8>(), 64) // upper bound
            .cast();
        gl21::BindAttribLocation(program, index, name);
    });
}
fn glGetActiveUniform(
    env: &mut Environment,
    program: GLuint,
    index: GLuint,
    buf_size: GLsizei,
    length: MutPtr<GLsizei>,
    size: MutPtr<GLint>,
    type_: MutPtr<GLenum>,
    name: MutPtr<u8>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let length = mem.ptr_at_mut(length, 1);
        let size = mem.ptr_at_mut(size, 1);
        let type_ = mem.ptr_at_mut(type_, 1);
        let name_buf_size = buf_size.unsigned_abs().max(1);
        let mut name_buf = vec![0u8; name_buf_size as usize];
        gl21::GetActiveUniform(
            program,
            index,
            name_buf_size as _,
            length.cast(),
            size.cast(),
            type_.cast(),
            name_buf.as_mut_ptr() as *mut gl21::types::GLchar,
        );
        if !name.is_null() && buf_size > 0 {
            let written = name_buf
                .iter()
                .position(|&b| b == 0)
                .map(|p| p as GuestUSize)
                .unwrap_or(name_buf_size - 1)
                .min(buf_size.unsigned_abs() - 1);
            let dest = mem.ptr_at_mut(name, written + 1);
            std::ptr::copy_nonoverlapping(name_buf.as_ptr(), dest, written as usize);
            *dest.add(written as usize) = 0;
        }
    });
}
fn glGetActiveAttrib(
    env: &mut Environment,
    program: GLuint,
    index: GLuint,
    buf_size: GLsizei,
    length: MutPtr<GLsizei>,
    size: MutPtr<GLint>,
    type_: MutPtr<GLenum>,
    name: MutPtr<u8>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let length = mem.ptr_at_mut(length, 1);
        let size = mem.ptr_at_mut(size, 1);
        let type_ = mem.ptr_at_mut(type_, 1);
        let name_buf_size = buf_size.unsigned_abs().max(1);
        let mut name_buf = vec![0u8; name_buf_size as usize];
        gl21::GetActiveAttrib(
            program,
            index,
            name_buf_size as _,
            length.cast(),
            size.cast(),
            type_.cast(),
            name_buf.as_mut_ptr() as *mut gl21::types::GLchar,
        );
        if !name.is_null() && buf_size > 0 {
            let written = name_buf
                .iter()
                .position(|&b| b == 0)
                .map(|p| p as GuestUSize)
                .unwrap_or(name_buf_size - 1)
                .min(buf_size.unsigned_abs() - 1);
            let dest = mem.ptr_at_mut(name, written + 1);
            std::ptr::copy_nonoverlapping(name_buf.as_ptr(), dest, written as usize);
            *dest.add(written as usize) = 0;
        }
    });
}
fn glUseProgram(env: &mut Environment, program: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::UseProgram(program) });
}
fn glUniform1f(env: &mut Environment, location: GLint, v0: GLfloat) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::Uniform1f(location, v0) });
}
fn glUniform2f(env: &mut Environment, location: GLint, v0: GLfloat, v1: GLfloat) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::Uniform2f(location, v0, v1)
    });
}
fn glUniform3f(env: &mut Environment, location: GLint, v0: GLfloat, v1: GLfloat, v2: GLfloat) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::Uniform3f(location, v0, v1, v2)
    });
}
fn glUniform4f(
    env: &mut Environment,
    location: GLint,
    v0: GLfloat,
    v1: GLfloat,
    v2: GLfloat,
    v3: GLfloat,
) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::Uniform4f(location, v0, v1, v2, v3)
    });
}
fn glUniform1i(env: &mut Environment, location: GLint, v0: GLint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::Uniform1i(location, v0) });
}
fn glUniform2i(env: &mut Environment, location: GLint, v0: GLint, v1: GLint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::Uniform2i(location, v0, v1)
    });
}
fn glUniform3i(env: &mut Environment, location: GLint, v0: GLint, v1: GLint, v2: GLint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::Uniform3i(location, v0, v1, v2)
    });
}
fn glUniform4i(env: &mut Environment, location: GLint, v0: GLint, v1: GLint, v2: GLint, v3: GLint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::Uniform4i(location, v0, v1, v2, v3)
    });
}
fn glUniform1fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() as _);
        gl21::Uniform1fv(location, count, value.cast());
    });
}
fn glUniform2fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() * 2);
        gl21::Uniform2fv(location, count, value.cast());
    });
}
fn glUniform3fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() * 3);
        gl21::Uniform3fv(location, count, value.cast());
    });
}
fn glUniform4fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() * 4);
        gl21::Uniform4fv(location, count, value.cast());
    });
}
fn glUniform1iv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() as _);
        gl21::Uniform1iv(location, count, value.cast());
    });
}
fn glUniform2iv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() * 2);
        gl21::Uniform2iv(location, count, value.cast());
    });
}
fn glUniform3iv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() * 3);
        gl21::Uniform3iv(location, count, value.cast());
    });
}
fn glUniform4iv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() * 4);
        gl21::Uniform4iv(location, count, value.cast());
    });
}
fn glUniformMatrix2fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() * 4);
        gl21::UniformMatrix2fv(location, count, transpose, value.cast());
    });
}
fn glUniformMatrix3fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() * 9);
        gl21::UniformMatrix3fv(location, count, transpose, value.cast());
    });
}
fn glUniformMatrix4fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let value = mem.ptr_at(value, count.unsigned_abs() * 16);
        gl21::UniformMatrix4fv(location, count, transpose, value.cast());
    });
}
fn glGetUniformfv(
    env: &mut Environment,
    program: GLuint,
    location: GLint,
    params: MutPtr<GLfloat>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at_mut(params, 16 /* upper bound */);
        gl21::GetUniformfv(program, location, params.cast());
    });
}
fn glGetUniformiv(env: &mut Environment, program: GLuint, location: GLint, params: MutPtr<GLint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at_mut(params, 16 /* upper bound */);
        gl21::GetUniformiv(program, location, params.cast());
    });
}
fn glEnableVertexAttribArray(env: &mut Environment, index: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        if index == GLuint::MAX {
            return;
        }
        gl21::EnableVertexAttribArray(index);
    });
}
fn glDisableVertexAttribArray(env: &mut Environment, index: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        if index == GLuint::MAX {
            return;
        }
        gl21::DisableVertexAttribArray(index);
    });
}
fn glVertexAttribPointer(
    env: &mut Environment,
    index: GLuint,
    size: GLint,
    type_: GLenum,
    normalized: GLboolean,
    stride: GLsizei,
    pointer: ConstVoidPtr,
) {
    with_gles2_and_mem(env, |gles, mem| unsafe {
        let pointer =
            translate_pointer_or_offset_to_host(gles, mem, pointer, gles11::ARRAY_BUFFER_BINDING);
        gl21::VertexAttribPointer(index, size, type_, normalized, stride, pointer.cast());
    });
}
fn glVertexAttrib1f(env: &mut Environment, index: GLuint, x: GLfloat) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::VertexAttrib1f(index, x) });
}
fn glVertexAttrib2f(env: &mut Environment, index: GLuint, x: GLfloat, y: GLfloat) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::VertexAttrib2f(index, x, y)
    });
}
fn glVertexAttrib3f(env: &mut Environment, index: GLuint, x: GLfloat, y: GLfloat, z: GLfloat) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::VertexAttrib3f(index, x, y, z)
    });
}
fn glVertexAttrib4f(
    env: &mut Environment,
    index: GLuint,
    x: GLfloat,
    y: GLfloat,
    z: GLfloat,
    w: GLfloat,
) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::VertexAttrib4f(index, x, y, z, w)
    });
}
fn glGetVertexAttribfv(
    env: &mut Environment,
    index: GLuint,
    pname: GLenum,
    params: MutPtr<GLfloat>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at_mut(params, 4);
        gl21::GetVertexAttribfv(index, pname, params.cast());
    });
}
fn glGetVertexAttribiv(env: &mut Environment, index: GLuint, pname: GLenum, params: MutPtr<GLint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at_mut(params, 4);
        gl21::GetVertexAttribiv(index, pname, params.cast());
    });
}

// Drawing

fn glDrawArrays(env: &mut Environment, mode: GLenum, first: GLint, count: GLsizei) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::DrawArrays(mode, first, count);
    });
}
fn glDrawElements(
    env: &mut Environment,
    mode: GLenum,
    count: GLsizei,
    type_: GLenum,
    indices: ConstVoidPtr,
) {
    with_gles2_and_mem(env, |gles, mem| unsafe {
        let indices = translate_pointer_or_offset_to_host(
            gles,
            mem,
            indices,
            gles11::ELEMENT_ARRAY_BUFFER_BINDING,
        );
        gl21::DrawElements(mode, count, type_, indices);
    });
}

// Framebuffers and renderbuffers

fn glGenFramebuffers(env: &mut Environment, n: GLsizei, framebuffers: MutPtr<GLuint>) {
    with_gles2_and_mem(env, |gles, mem| unsafe {
        let framebuffers = mem.ptr_at_mut(framebuffers, n.unsigned_abs() as _);
        gl21::GenFramebuffersEXT(n, framebuffers.cast());
        for i in 0..n.unsigned_abs() as usize {
            // Record that this name is a real framebuffer (not a renderbuffer
            // used under the EAGL idiom): desktop GL keeps FBO and RBO names
            // in separate namespaces, so a guest FBO can share its numeric
            // name with a host renderbuffer, which would otherwise be
            // misinterpreted as an EAGL-idiom bind.
            gles.record_guest_fbo_name(*framebuffers.add(i));
        }
    });
}
fn glDeleteFramebuffers(env: &mut Environment, n: GLsizei, framebuffers: ConstPtr<GLuint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let framebuffers = mem.ptr_at(framebuffers, n.unsigned_abs() as _);
        gl21::DeleteFramebuffersEXT(n, framebuffers.cast());
    });
}
fn glBindFramebuffer(env: &mut Environment, target: GLenum, framebuffer: GLuint) {
    with_gles2_and_mem(env, |gles, _mem| gles.bind_framebuffer(target, framebuffer));
}
fn glIsFramebuffer(env: &mut Environment, framebuffer: GLuint) -> GLboolean {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::IsFramebufferEXT(framebuffer)
    })
}
fn glCheckFramebufferStatus(env: &mut Environment, target: GLenum) -> GLenum {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::CheckFramebufferStatusEXT(target)
    })
}
fn glFramebufferRenderbuffer(
    env: &mut Environment,
    target: GLenum,
    attachment: GLenum,
    renderbuffertarget: GLenum,
    renderbuffer: GLuint,
) {
    with_gles2_and_mem(env, |gles, _mem| unsafe {
        // Record the guest's color<->depth/stencil renderbuffer pairing: the
        // EAGL idiom passes the *color* renderbuffer's name as the framebuffer
        // and attaches the depth/stencil renderbuffer to it. Needed so the
        // wrapper FBO we create for the color renderbuffer gets stencil too.
        let current_guest_fb = gles.current_guest_framebuffer();
        if let Some(pairs) = gles.default_framebuffer_pairing_mut() {
            match attachment {
                0x8d00 /* GL_DEPTH_ATTACHMENT */ | 0x8d30 /* GL_DEPTH_STENCIL_ATTACHMENT */ | 0x8d20 /* GL_STENCIL_ATTACHMENT */ => {
                    pairs.insert(current_guest_fb, renderbuffer);
                }
                _ => {}
            }
        }
        gl21::FramebufferRenderbufferEXT(target, attachment, renderbuffertarget, renderbuffer)
    });
}
fn glFramebufferTexture2D(
    env: &mut Environment,
    target: GLenum,
    attachment: GLenum,
    textarget: GLenum,
    texture: GLuint,
    level: GLint,
) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::FramebufferTexture2DEXT(target, attachment, textarget, texture, level)
    });
}
fn glGenRenderbuffers(env: &mut Environment, n: GLsizei, renderbuffers: MutPtr<GLuint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let renderbuffers = mem.ptr_at_mut(renderbuffers, n.unsigned_abs() as _);
        gl21::GenRenderbuffersEXT(n, renderbuffers.cast());
    });
}
fn glDeleteRenderbuffers(env: &mut Environment, n: GLsizei, renderbuffers: ConstPtr<GLuint>) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let renderbuffers = mem.ptr_at(renderbuffers, n.unsigned_abs() as _);
        gl21::DeleteRenderbuffersEXT(n, renderbuffers.cast());
    });
}
fn glBindRenderbuffer(env: &mut Environment, target: GLenum, renderbuffer: GLuint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::BindRenderbufferEXT(target, renderbuffer)
    });
}
fn glIsRenderbuffer(env: &mut Environment, renderbuffer: GLuint) -> GLboolean {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::IsRenderbufferEXT(renderbuffer)
    })
}
fn glRenderbufferStorage(
    env: &mut Environment,
    target: GLenum,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        // Map ES 2.0 internal formats to desktop GL equivalents.
        let internalformat = match internalformat {
            0x81a4 /* GL_DEPTH_COMPONENT16 */ => gl21::DEPTH_COMPONENT,
            0x8d48 /* GL_STENCIL_INDEX8 */ => gl21::STENCIL_INDEX1_EXT,
            0x8d62 /* GL_RGB565 */ => gl21::RGB,
            _ => internalformat,
        };
        gl21::RenderbufferStorageEXT(target, internalformat, width, height)
    });
}
fn glGetRenderbufferParameteriv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    with_gles2_and_mem(env, |_gles, mem| unsafe {
        let params = mem.ptr_at_mut(params, 4);
        gl21::GetRenderbufferParameterivEXT(target, pname, params.cast());
    });
}
fn glGenerateMipmap(env: &mut Environment, target: GLenum) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::GenerateMipmapEXT(target)
    });
}

// Clearing

fn glClear(env: &mut Environment, mask: GLbitfield) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::Clear(mask) });
}
fn glClearColor(
    env: &mut Environment,
    red: GLclampf,
    green: GLclampf,
    blue: GLclampf,
    alpha: GLclampf,
) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe {
        gl21::ClearColor(red, green, blue, alpha)
    });
}
fn glClearDepthf(env: &mut Environment, depth: GLclampf) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::ClearDepth(depth as f64) });
}
fn glClearStencil(env: &mut Environment, s: GLint) {
    with_gles2_and_mem(env, |_gles, _mem| unsafe { gl21::ClearStencil(s) });
}

// Miscellaneous stubs

fn glShaderBinary(
    _env: &mut Environment,
    _n: GLsizei,
    _shaders: ConstPtr<GLuint>,
    _binary_format: GLenum,
    _binary: ConstVoidPtr,
    _length: GLsizei,
) {
    log!("Warning: glShaderBinary() is not supported, ignoring");
}
fn glRenderbufferStorageMultisampleAPPLE(
    _env: &mut Environment,
    _target: GLenum,
    _samples: GLsizei,
    _internalformat: GLenum,
    _width: GLsizei,
    _height: GLsizei,
) {
    log!("Warning: glRenderbufferStorageMultisampleAPPLE() is not supported, ignoring");
}

pub const FUNCTIONS: FunctionExports = &[
    // Generic state manipulation
    export_c_func!(glGetError()),
    export_c_func!(glEnable(_)),
    export_c_func!(glDisable(_)),
    export_c_func!(glIsEnabled(_)),
    export_c_func!(glGetIntegerv(_, _)),
    export_c_func!(glGetFloatv(_, _)),
    export_c_func!(glGetString(_)),
    export_c_func!(glHint(_, _)),
    export_c_func!(glFinish()),
    export_c_func!(glFlush()),
    // Other state manipulation
    export_c_func!(glBlendFunc(_, _)),
    export_c_func!(glBlendColor(_, _, _, _)),
    export_c_func!(glBlendEquation(_)),
    export_c_func!(glBlendFuncSeparate(_, _, _, _)),
    export_c_func!(glColorMask(_, _, _, _)),
    export_c_func!(glCullFace(_)),
    export_c_func!(glDepthFunc(_)),
    export_c_func!(glDepthMask(_)),
    export_c_func!(glDepthRangef(_, _)),
    export_c_func!(glFrontFace(_)),
    export_c_func!(glLineWidth(_)),
    export_c_func!(glPolygonOffset(_, _)),
    export_c_func!(glSampleCoverage(_, _)),
    export_c_func!(glScissor(_, _, _, _)),
    export_c_func!(glStencilFunc(_, _, _)),
    export_c_func!(glStencilFuncSeparate(_, _, _, _)),
    export_c_func!(glStencilMask(_)),
    export_c_func!(glStencilMaskSeparate(_, _)),
    export_c_func!(glStencilOp(_, _, _)),
    export_c_func!(glStencilOpSeparate(_, _, _, _)),
    export_c_func!(glViewport(_, _, _, _)),
    // Buffers
    export_c_func!(glGenBuffers(_, _)),
    export_c_func!(glDeleteBuffers(_, _)),
    export_c_func!(glBindBuffer(_, _)),
    export_c_func!(glBufferData(_, _, _, _)),
    export_c_func!(glBufferSubData(_, _, _, _)),
    export_c_func!(glGetBufferParameteriv(_, _, _)),
    // Textures
    export_c_func!(glGenTextures(_, _)),
    export_c_func!(glDeleteTextures(_, _)),
    export_c_func!(glActiveTexture(_)),
    export_c_func!(glBindTexture(_, _)),
    export_c_func!(glIsTexture(_)),
    export_c_func!(glTexParameteri(_, _, _)),
    export_c_func!(glTexParameterf(_, _, _)),
    export_c_func!(glTexParameteriv(_, _, _)),
    export_c_func!(glTexParameterfv(_, _, _)),
    export_c_func!(glTexImage2D(_, _, _, _, _, _, _, _, _)),
    export_c_func!(glTexSubImage2D(_, _, _, _, _, _, _, _, _)),
    export_c_func!(glCompressedTexImage2D(_, _, _, _, _, _, _, _)),
    export_c_func!(glPixelStorei(_, _)),
    export_c_func!(glReadPixels(_, _, _, _, _, _, _)),
    // Shaders and programs
    export_c_func!(glCreateShader(_)),
    export_c_func!(glDeleteShader(_)),
    export_c_func!(glIsShader(_)),
    export_c_func!(glShaderSource(_, _, _, _)),
    export_c_func!(glCompileShader(_)),
    export_c_func!(glGetShaderiv(_, _, _)),
    export_c_func!(glGetShaderInfoLog(_, _, _, _)),
    export_c_func!(glGetShaderSource(_, _, _, _)),
    export_c_func!(glReleaseShaderCompiler()),
    export_c_func!(glGetShaderPrecisionFormat(_, _, _, _)),
    export_c_func!(glCreateProgram()),
    export_c_func!(glDeleteProgram(_)),
    export_c_func!(glIsProgram(_)),
    export_c_func!(glAttachShader(_, _)),
    export_c_func!(glDetachShader(_, _)),
    export_c_func!(glLinkProgram(_)),
    export_c_func!(glValidateProgram(_)),
    export_c_func!(glGetProgramiv(_, _, _)),
    export_c_func!(glGetProgramInfoLog(_, _, _, _)),
    export_c_func!(glGetAttachedShaders(_, _, _, _)),
    // Uniforms and attributes
    export_c_func!(glGetUniformLocation(_, _)),
    export_c_func!(glGetAttribLocation(_, _)),
    export_c_func!(glBindAttribLocation(_, _, _)),
    export_c_func!(glGetActiveUniform(_, _, _, _, _, _, _)),
    export_c_func!(glGetActiveAttrib(_, _, _, _, _, _, _)),
    export_c_func!(glUseProgram(_)),
    export_c_func!(glUniform1f(_, _)),
    export_c_func!(glUniform2f(_, _, _)),
    export_c_func!(glUniform3f(_, _, _, _)),
    export_c_func!(glUniform4f(_, _, _, _, _)),
    export_c_func!(glUniform1i(_, _)),
    export_c_func!(glUniform2i(_, _, _)),
    export_c_func!(glUniform3i(_, _, _, _)),
    export_c_func!(glUniform4i(_, _, _, _, _)),
    export_c_func!(glUniform1fv(_, _, _)),
    export_c_func!(glUniform2fv(_, _, _)),
    export_c_func!(glUniform3fv(_, _, _)),
    export_c_func!(glUniform4fv(_, _, _)),
    export_c_func!(glUniform1iv(_, _, _)),
    export_c_func!(glUniform2iv(_, _, _)),
    export_c_func!(glUniform3iv(_, _, _)),
    export_c_func!(glUniform4iv(_, _, _)),
    export_c_func!(glUniformMatrix2fv(_, _, _, _)),
    export_c_func!(glUniformMatrix3fv(_, _, _, _)),
    export_c_func!(glUniformMatrix4fv(_, _, _, _)),
    export_c_func!(glGetUniformfv(_, _, _)),
    export_c_func!(glGetUniformiv(_, _, _)),
    export_c_func!(glEnableVertexAttribArray(_)),
    export_c_func!(glDisableVertexAttribArray(_)),
    export_c_func!(glVertexAttribPointer(_, _, _, _, _, _)),
    export_c_func!(glVertexAttrib1f(_, _)),
    export_c_func!(glVertexAttrib2f(_, _, _)),
    export_c_func!(glVertexAttrib3f(_, _, _, _)),
    export_c_func!(glVertexAttrib4f(_, _, _, _, _)),
    export_c_func!(glGetVertexAttribfv(_, _, _)),
    export_c_func!(glGetVertexAttribiv(_, _, _)),
    // Drawing
    export_c_func!(glDrawArrays(_, _, _)),
    export_c_func!(glDrawElements(_, _, _, _)),
    // Framebuffers and renderbuffers
    export_c_func!(glGenFramebuffers(_, _)),
    export_c_func!(glDeleteFramebuffers(_, _)),
    export_c_func!(glBindFramebuffer(_, _)),
    export_c_func!(glIsFramebuffer(_)),
    export_c_func!(glCheckFramebufferStatus(_)),
    export_c_func!(glFramebufferRenderbuffer(_, _, _, _)),
    export_c_func!(glFramebufferTexture2D(_, _, _, _, _)),
    export_c_func!(glGenRenderbuffers(_, _)),
    export_c_func!(glDeleteRenderbuffers(_, _)),
    export_c_func!(glBindRenderbuffer(_, _)),
    export_c_func!(glIsRenderbuffer(_)),
    export_c_func!(glRenderbufferStorage(_, _, _, _)),
    export_c_func!(glGetRenderbufferParameteriv(_, _, _)),
    export_c_func!(glGenerateMipmap(_)),
    // Clearing
    export_c_func!(glClear(_)),
    export_c_func!(glClearColor(_, _, _, _)),
    export_c_func!(glClearDepthf(_)),
    export_c_func!(glClearStencil(_)),
    // Miscellaneous stubs
    export_c_func!(glShaderBinary(_, _, _, _, _)),
    export_c_func!(glRenderbufferStorageMultisampleAPPLE(_, _, _, _, _)),
];
