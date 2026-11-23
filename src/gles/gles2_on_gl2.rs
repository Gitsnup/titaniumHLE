/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Implementation of OpenGL ES 2.0 on top of OpenGL 2.1 compatibility profile.
//!
//! OpenGL ES 2.0 is a shader-based API that shares most of its entry points
//! and semantics with desktop OpenGL 2.0+. Rather than re-implementing a
//! driver, we forward guest calls to the host's OpenGL implementation:
//!
//! - Almost all OpenGL ES 2.0 functions exist in desktop OpenGL 2.1 (via
//!   `EXT_framebuffer_object`) with the same names and semantics.
//! - Guest GLSL ES 1.00 shaders are preprocessed (see
//!   [crate::gles::gles2_on_gl2::translate_shader_source] in the guest
//!   wrapper) so that desktop GLSL accepts them.
//! - The handful of ES 2.0-specific functions that have no desktop
//!   equivalent (`glGetShaderPrecisionFormat` etc.) are implemented in the
//!   guest-facing wrapper ([crate::frameworks::opengles::gles2_guest]).
//! - In EAGL, framebuffer object 0 *is* the renderbuffer that the app
//!   attached to its `CAEAGLLayer`. On desktop GL, framebuffer 0 is the
//!   window surface, which we can't read back from. We therefore give the
//!   guest an offscreen framebuffer ("the default framebuffer") whenever it
//!   binds 0, and blit it into the real renderbuffer on
//!   `presentRenderbuffer:`. See [Gles2DefaultFramebuffer].
//!
//! The shared GL context is wrapped by a [GLES1OnGL2Context] (see
//! [GLES2OnGL2Context::inner]), which is used to implement the
//! [crate::gles::GLESContext] trait. This gives us, for free, the
//! fixed-function blit used to present frames (and any other internal code
//! that consumes `Box<dyn GLES>`), which is safe to run in the same context
//! because the adapter saves and restores the guest's shader state.

use super::gles1_on_gl2::GLES1OnGL2Context;
use super::gles_generic::GLESContext;
use super::GLES;
use crate::gles::gl21compat_raw::types::{GLenum, GLint, GLsizei, GLuint};
use crate::mem::GuestUSize;
use crate::window::{GLContext, GLVersion, Window};
use touchHLE_gl_bindings::gl21compat as gl21;
use touchHLE_gl_bindings::gl30;

/// Emulated ES2 "default framebuffer" (framebuffer object 0).
///
/// In EAGL, when an app binds framebuffer 0 and renders, the rendering goes
/// to the renderbuffer it attached to its `CAEAGLLayer`. Desktop GL has no
/// such concept (framebuffer 0 is the on-screen window surface, which we
/// cannot read back from for compositing). We therefore allocate an offscreen
/// color renderbuffer (plus a depth/stencil renderbuffer, which the default
/// framebuffer always has in ES) whenever the guest binds framebuffer 0, and
/// blit from it into the real drawable renderbuffer on present.
#[derive(Default)]
pub struct Gles2DefaultFramebuffer {
    /// Host framebuffer object standing in for "framebuffer 0".
    /// Zero means it has not been created yet.
    fbo: GLuint,
    color_renderbuffer: GLuint,
    depth_stencil_renderbuffer: GLuint,
    /// Size the default framebuffer was last allocated at.
    size: (GuestUSize, GuestUSize),
    /// The guest-visible framebuffer binding (what the guest last passed to
    /// `glBindFramebuffer` for `GL_FRAMEBUFFER`/`GL_DRAW_FRAMEBUFFER`).
    /// Zero means the guest is rendering to the (emulated) default
    /// framebuffer.
    current_guest_framebuffer: GLuint,
    /// Wrapper FBOs for renderbuffers that the guest binds as framebuffers
    /// (the EAGL idiom: the drawable renderbuffer *is* the framebuffer).
    framebuffer_wrappers: std::collections::HashMap<GLuint, GLuint>,
    /// The guest's color<->depth/stencil renderbuffer pairing, as recorded
    /// from `glFramebufferRenderbuffer` calls (EAGL idiom: the color
    /// renderbuffer's name is used as the framebuffer).
    color_stencil_pairs: std::collections::HashMap<GLuint, GLuint>,
    /// Framebuffer names the guest created via `glGenFramebuffers`. Used to
    /// distinguish guest FBO binds from EAGL-idiom binds of renderbuffer
    /// names (desktop GL keeps FBO and RBO names in separate namespaces, so
    /// a guest FBO and a host renderbuffer can share the same numeric name).
    guest_fbo_names: std::collections::HashSet<GLuint>,
}

impl Gles2DefaultFramebuffer {
    /// Make sure the backing storage exists and matches `size`.
    fn allocate(&mut self, size: (GuestUSize, GuestUSize)) {
        let (width, height) = size;
        if self.fbo != 0 && self.size == size {
            return;
        }
        unsafe {
            if self.fbo == 0 {
                let mut fbo = 0;
                gl30::GenFramebuffers(1, &mut fbo);
                let mut rb = 0;
                gl30::GenRenderbuffers(1, &mut rb);
                let mut ds = 0;
                gl30::GenRenderbuffers(1, &mut ds);
                self.fbo = fbo;
                self.color_renderbuffer = rb;
                self.depth_stencil_renderbuffer = ds;
            }
            let old_binding = self.get_binding(gl30::DRAW_FRAMEBUFFER_BINDING);
            gl30::BindFramebuffer(gl30::FRAMEBUFFER, self.fbo);
            gl30::BindRenderbuffer(gl30::RENDERBUFFER, self.color_renderbuffer);
            gl30::RenderbufferStorage(
                gl30::RENDERBUFFER,
                gl21::RGBA8,
                width as GLsizei,
                height as GLsizei,
            );
            gl30::BindRenderbuffer(gl30::RENDERBUFFER, self.depth_stencil_renderbuffer);
            gl30::RenderbufferStorage(
                gl30::RENDERBUFFER,
                gl30::DEPTH24_STENCIL8,
                width as GLsizei,
                height as GLsizei,
            );
            gl30::BindRenderbuffer(gl30::RENDERBUFFER, 0);
            gl30::FramebufferRenderbuffer(
                gl30::FRAMEBUFFER,
                gl30::COLOR_ATTACHMENT0,
                gl30::RENDERBUFFER,
                self.color_renderbuffer,
            );
            gl30::FramebufferRenderbuffer(
                gl30::FRAMEBUFFER,
                gl30::DEPTH_STENCIL_ATTACHMENT,
                gl30::RENDERBUFFER,
                self.depth_stencil_renderbuffer,
            );
            let status = gl30::CheckFramebufferStatus(gl30::FRAMEBUFFER);
            if status != gl30::FRAMEBUFFER_COMPLETE {
                log!(
                    "Warning: GLES2 default framebuffer is incomplete (status {:#x}, {}x{})",
                    status,
                    width,
                    height
                );
            }
            gl30::BindFramebuffer(gl30::FRAMEBUFFER, old_binding as GLuint);
        }
        self.size = size;
    }

    /// Handle a guest `glBindFramebuffer` call for `target`.
    ///
    /// Framebuffer 0 is remapped to the emulated default framebuffer; other
    /// names pass through unchanged. The guest-visible binding is tracked so
    /// that it survives context activation switches.
    fn bind(&mut self, target: GLenum, framebuffer: GLuint) {
        match target {
            x if x == gl30::FRAMEBUFFER || x == gl30::DRAW_FRAMEBUFFER => {
                self.current_guest_framebuffer = framebuffer;
            }
            _ => {}
        }
        let size = unsafe {
            let rb = self.get_binding(gl21::RENDERBUFFER_BINDING_EXT);
            if rb == 0 {
                (0, 0)
            } else {
                (
                    self.get_renderbuffer_parameter(rb as GLuint, gl21::RENDERBUFFER_WIDTH_EXT)
                        as _,
                    self.get_renderbuffer_parameter(rb as GLuint, gl21::RENDERBUFFER_HEIGHT_EXT)
                        as _,
                )
            }
        };
        if framebuffer == 0 && size != (0, 0) {
            self.allocate(size);
        }
        let host_framebuffer = if framebuffer == 0 && self.fbo != 0 {
            self.fbo
        } else {
            framebuffer
        };
        unsafe {
            gl21::BindFramebufferEXT(target, host_framebuffer);
        }
    }

    /// Get (or lazily create) a framebuffer object with `renderbuffer`
    /// attached, for EAGL-style "bind the drawable renderbuffer as the
    /// framebuffer" usage. Depth-format renderbuffers are attached as the
    /// depth attachment instead of the color attachment.
    fn wrapper_fbo_for_renderbuffer(&mut self, renderbuffer: GLuint) -> GLuint {
        if let Some(&fbo) = self.framebuffer_wrappers.get(&renderbuffer) {
            return fbo;
        }
        let fbo = unsafe {
            let mut fbo = 0;
            gl30::GenFramebuffers(1, &mut fbo);
            // Determine whether this renderbuffer holds color or depth data.
            gl21::BindRenderbufferEXT(gl21::RENDERBUFFER_EXT, renderbuffer);
            let mut internal_format = 0;
            gl21::GetRenderbufferParameterivEXT(
                gl21::RENDERBUFFER_EXT,
                0x8D44, /* GL_RENDERBUFFER_INTERNAL_FORMAT_OES */
                &mut internal_format,
            );
            let is_depth = internal_format == gl21::DEPTH_COMPONENT as GLint
                || internal_format == gl21::DEPTH_COMPONENT16 as GLint
                || internal_format == 0x81a5 /* GL_DEPTH_COMPONENT24 */
                || internal_format == 0x81a6 /* GL_DEPTH_COMPONENT32 */
                || internal_format == gl30::DEPTH_COMPONENT16 as GLint;
            gl21::BindFramebufferEXT(gl21::FRAMEBUFFER_EXT, fbo);
            if is_depth {
                gl21::FramebufferRenderbufferEXT(
                    gl21::FRAMEBUFFER_EXT,
                    gl30::DEPTH_ATTACHMENT,
                    gl21::RENDERBUFFER_EXT,
                    renderbuffer,
                );
            } else {
                gl21::FramebufferRenderbufferEXT(
                    gl21::FRAMEBUFFER_EXT,
                    gl30::COLOR_ATTACHMENT0,
                    gl21::RENDERBUFFER_EXT,
                    renderbuffer,
                );
            }
            let stencil_rb = self
                .color_stencil_pairs
                .get(&renderbuffer)
                .copied()
                .unwrap_or(0);
            if stencil_rb != 0 {
                gl21::FramebufferRenderbufferEXT(
                    gl21::FRAMEBUFFER_EXT,
                    gl30::STENCIL_ATTACHMENT,
                    gl21::RENDERBUFFER_EXT,
                    stencil_rb,
                );
            }
            gl21::BindFramebufferEXT(gl21::FRAMEBUFFER_EXT, 0);
            fbo
        };
        self.framebuffer_wrappers.insert(renderbuffer, fbo);
        fbo
    }

    /// Re-apply the guest-visible framebuffer binding after a context
    /// activation (the real GL binding may have been changed by internal
    /// code or by another context).
    fn sync_on_activate(&mut self) {
        // The guest shares this GL context with touchHLE's host-side GLES
        // machinery (the framebuffer-0 blit, the GLES1 wrapper's shader-state
        // save/restore, the compositor). Those calls can record GL errors,
        // which would otherwise leak into the guest's sticky error state and
        // get misattributed to guest calls (on real iOS, ES1 and ES2 lived in
        // separate contexts, so this couldn't happen). Drain the error flags
        // here so each guest call starts clean.
        unsafe {
            let mut i = 0;
            while gl21::GetError() != 0 && i < 16 {
                i += 1;
            }
        }
        let guest = self.current_guest_framebuffer;
        if guest == 0 && self.fbo == 0 {
            // Fresh context: desktop framebuffer 0 is bound, which matches
            // the guest's view of things (until the emulated default
            // framebuffer is created).
            return;
        }
        if guest != 0 && !self.guest_fbo_names.contains(&guest) {
            // EAGL idiom: re-bind the wrapper FBO for the renderbuffer.
            let fbo = self.wrapper_fbo_for_renderbuffer(guest);
            unsafe {
                gl21::BindFramebufferEXT(gl30::FRAMEBUFFER, fbo);
            }
        } else {
            self.bind(gl30::FRAMEBUFFER, guest);
        }
    }

    unsafe fn get_binding(&self, pname: GLenum) -> GLint {
        let mut value = 0;
        gl21::GetIntegerv(pname, &mut value);
        value
    }

    unsafe fn get_renderbuffer_parameter(&self, renderbuffer: GLuint, pname: GLenum) -> GLint {
        let mut value = 0;
        gl21::BindRenderbufferEXT(gl21::RENDERBUFFER_EXT, renderbuffer);
        gl21::GetRenderbufferParameterivEXT(gl21::RENDERBUFFER_EXT, pname, &mut value);
        value
    }

    /// Blit the emulated default framebuffer into `renderbuffer`, which
    /// should be the renderbuffer currently bound to `GL_RENDERBUFFER`
    /// (i.e. the drawable's renderbuffer, as far as the guest is concerned).
    fn blit_to_renderbuffer(&mut self, renderbuffer: GLuint) {
        if self.current_guest_framebuffer == renderbuffer {
            // EAGL idiom: the drawable's renderbuffer *is* the guest's draw
            // target, so the frame content is already in it.
            return;
        }
        if self.fbo == 0 {
            // The guest never rendered to the default framebuffer.
            return;
        }
        let (width, height) = unsafe {
            (
                self.get_renderbuffer_parameter(renderbuffer, gl21::RENDERBUFFER_WIDTH_EXT)
                    as GuestUSize,
                self.get_renderbuffer_parameter(renderbuffer, gl21::RENDERBUFFER_HEIGHT_EXT)
                    as GuestUSize,
            )
        };
        if width == 0 || height == 0 {
            return;
        }
        self.allocate((width, height));
        unsafe {
            let old_draw = self.get_binding(gl30::DRAW_FRAMEBUFFER_BINDING);
            let old_read = self.get_binding(gl30::READ_FRAMEBUFFER_BINDING);
            // Attach the drawable's renderbuffer to a temporary framebuffer
            // and blit into it. Reuse the emulated default framebuffer's
            // sibling FBO trick: attach to a fresh FBO each present, it's
            // cheap (the renderbuffer itself is untouched).
            let mut dst_fbo = 0;
            gl30::GenFramebuffers(1, &mut dst_fbo);
            gl30::BindFramebuffer(gl30::DRAW_FRAMEBUFFER, dst_fbo);
            gl30::FramebufferRenderbuffer(
                gl30::DRAW_FRAMEBUFFER,
                gl30::COLOR_ATTACHMENT0,
                gl30::RENDERBUFFER,
                renderbuffer,
            );
            gl30::BindFramebuffer(gl30::READ_FRAMEBUFFER, self.fbo);
            gl30::BlitFramebuffer(
                0,
                0,
                self.size.0 as GLint,
                self.size.1 as GLint,
                0,
                0,
                width as GLint,
                height as GLint,
                gl30::COLOR_BUFFER_BIT,
                gl30::LINEAR,
            );
            gl30::DeleteFramebuffers(1, &dst_fbo);
            // Restore the guest's binding state ("framebuffer 0").
            gl30::BindFramebuffer(gl30::READ_FRAMEBUFFER, old_read as GLuint);
            gl30::BindFramebuffer(gl30::DRAW_FRAMEBUFFER, old_draw as GLuint);
        }
    }
}

pub struct GLES2OnGL2Context {
    /// Runs the fixed-function presentation blit (and any other
    /// [GLES]-trait consumers) in the same GL context as the guest.
    pub(crate) inner: GLES1OnGL2Context,
    /// State for the emulated ES2 default framebuffer (framebuffer 0).
    default_framebuffer: Gles2DefaultFramebuffer,
}

impl GLES2OnGL2Context {
    pub fn new(window: &mut Window) -> Result<Self, String> {
        let gl_ctx: GLContext = window.create_gl_context(GLVersion::GL21Compat)?;
        Ok(Self {
            inner: GLES1OnGL2Context::new_with_context(gl_ctx),
            default_framebuffer: Gles2DefaultFramebuffer::default(),
        })
    }

    /// Make the shared GL context current for guest-facing GLES2 calls.
    /// Make the shared GL context current for guest-facing GLES2 calls.
    ///
    /// Unlike [GLESContext::make_current] (which prepares the context for a
    /// fixed-function blit and saves shader state), this leaves the guest's
    /// shader state untouched, since the guest is about to make its own GL
    /// calls.
    pub fn make_current_gles2<'gl_ctx, 'win: 'gl_ctx>(
        &'gl_ctx mut self,
        window: &'win mut Window,
    ) -> GLES2Active<'gl_ctx> {
        self.inner.make_gl_current(window);
        self.default_framebuffer.sync_on_activate();
        GLES2Active {
            default_framebuffer: &mut self.default_framebuffer,
        }
    }

    /// Blit the emulated default framebuffer into the drawable renderbuffer
    /// (the renderbuffer currently bound to `GL_RENDERBUFFER`), then read it
    /// back as RGBA for the RAM presentation path. Returns
    /// `(pixels, width, height)`, or `None` if the default framebuffer has
    /// never been used. Part of `presentRenderbuffer:`.
    pub fn present_and_read_default_framebuffer(
        &mut self,
        window: &mut Window,
    ) -> Option<(Vec<u8>, u32, u32)> {
        self.present_default_framebuffer(window);
        self.fb0_read_rgba(window)
    }

    pub fn fb0_read_rgba(&mut self, window: &mut Window) -> Option<(Vec<u8>, u32, u32)> {
        if self.default_framebuffer.fbo == 0 {
            return None;
        }
        let mut active = self.make_current_gles2(window);
        active.fb0_read_rgba()
    }

    pub fn present_default_framebuffer(&mut self, window: &mut Window) {
        let active = self.make_current_gles2(window);
        unsafe {
            let renderbuffer = active.get_binding(gl21::RENDERBUFFER_BINDING_EXT) as GLuint;
            active
                .default_framebuffer
                .blit_to_renderbuffer(renderbuffer);
        }
    }
}

impl GLESContext for GLES2OnGL2Context {
    fn description() -> &'static str {
        "OpenGL ES 2.0 via touchHLE GLES2-on-GL2 layer"
    }

    fn new(window: &mut Window) -> Result<Self, String> {
        Self::new(window)
    }

    fn make_current<'gl_ctx, 'win: 'gl_ctx>(
        &'gl_ctx mut self,
        window: &'win mut Window,
    ) -> Box<dyn GLES + 'gl_ctx> {
        self.inner.make_current(window)
    }

    unsafe fn make_current_unchecked_for_window<'gl_ctx>(
        &'gl_ctx mut self,
        make_current_fn: &mut dyn FnMut(&GLContext),
        loader_fn: &mut dyn FnMut(&'static str) -> *const std::ffi::c_void,
    ) -> Box<dyn GLES + 'gl_ctx> {
        self.inner
            .make_current_unchecked_for_window(make_current_fn, loader_fn)
    }

    fn as_gles2_on_gl2_mut(&mut self) -> Option<&mut GLES2OnGL2Context> {
        Some(self)
    }
}

/// An active GLES2 context. GL calls are made directly through the global
/// `gl21compat` bindings, which are loaded by
/// [GLES2OnGL2Context::make_current_gles2] and remain valid across contexts
/// from the same driver.
pub struct GLES2Active<'a> {
    pub(crate) default_framebuffer: &'a mut Gles2DefaultFramebuffer,
}

impl<'a> GLES2Active<'a> {
    /// Access to the guest's color<->depth/stencil renderbuffer pairing map
    /// (used by `glFramebufferRenderbuffer` under the EAGL idiom).
    pub fn default_framebuffer_pairing_mut(
        &mut self,
    ) -> Option<&mut std::collections::HashMap<GLuint, GLuint>> {
        Some(&mut self.default_framebuffer.color_stencil_pairs)
    }

    /// The guest-visible framebuffer binding (renderbuffer name under the
    /// EAGL idiom, or a guest-created FBO name, or 0).
    pub fn current_guest_framebuffer(&self) -> GLuint {
        self.default_framebuffer.current_guest_framebuffer
    }

    /// Handle a guest `glBindFramebuffer`/`glBindFramebufferOES` call.
    ///
    /// Framebuffer 0 is remapped to the emulated default framebuffer, and
    /// renderbuffer names bound as framebuffers (the EAGL idiom, where the
    /// drawable renderbuffer *is* the framebuffer) are wrapped in a generated
    /// framebuffer object that has the renderbuffer attached.
    pub fn bind_framebuffer(&mut self, target: GLenum, framebuffer: GLuint) {
        unsafe {
            if framebuffer != 0
                && !self
                    .default_framebuffer
                    .guest_fbo_names
                    .contains(&framebuffer)
                && gl21::IsRenderbufferEXT(framebuffer) != 0
            {
                // EAGL idiom: the guest bound a renderbuffer name (the
                // drawable's storage) as a framebuffer.
                let fbo = self
                    .default_framebuffer
                    .wrapper_fbo_for_renderbuffer(framebuffer);
                if target == gl30::FRAMEBUFFER || target == gl30::DRAW_FRAMEBUFFER {
                    self.default_framebuffer.current_guest_framebuffer = framebuffer;
                }
                gl21::BindFramebufferEXT(target, fbo);
            } else {
                self.default_framebuffer.bind(target, framebuffer);
            }
        }
    }

    /// Record a framebuffer name the guest created via `glGenFramebuffers`.
    pub fn record_guest_fbo_name(&mut self, framebuffer: GLuint) {
        self.default_framebuffer.guest_fbo_names.insert(framebuffer);
    }

    /// Read the emulated default framebuffer's pixels as RGBA (converts from
    /// the internal BGRA layout), for the layer-compositing present path.
    /// Returns `(pixels, width, height)`, or `None` if nothing was ever bound.
    pub fn fb0_read_rgba(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        let fb = &self.default_framebuffer;
        if fb.fbo == 0 {
            return None;
        }
        let (width, height) = fb.size;
        if width == 0 || height == 0 {
            return None;
        }
        let old_draw = unsafe {
            let mut v: GLint = 0;
            gl21::GetIntegerv(gl30::DRAW_FRAMEBUFFER_BINDING, &mut v);
            v
        };
        unsafe {
            gl30::BindFramebuffer(gl30::FRAMEBUFFER, fb.fbo);
            gl30::ReadBuffer(gl30::COLOR_ATTACHMENT0);
        }
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        unsafe {
            gl30::ReadPixels(
                0,
                0,
                width as GLsizei,
                height as GLsizei,
                gl30::BGRA,
                gl30::UNSIGNED_BYTE,
                pixels.as_mut_ptr() as *mut _,
            );
        }
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        unsafe { gl30::BindFramebuffer(gl30::FRAMEBUFFER, old_draw as GLuint) };
        Some((pixels, width, height))
    }

    unsafe fn get_binding(&self, pname: GLenum) -> GLint {
        let mut value = 0;
        gl21::GetIntegerv(pname, &mut value);
        value
    }
}
