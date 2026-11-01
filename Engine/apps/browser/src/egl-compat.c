/* wgpu links the EGL 1.5 entry points, while Emscripten exposes EGL 1.4.
 * Browser/offscreen contexts use the default display and the equivalent 1.4
 * window-surface operation. Reject attributes that cannot be represented. */
#include <EGL/egl.h>
#include <stdint.h>
#include <limits.h>

EGLDisplay eglGetPlatformDisplay(EGLenum platform, void *display, const EGLAttrib *attributes) {
    (void)platform;
    if (display != EGL_DEFAULT_DISPLAY || (attributes && attributes[0] != EGL_NONE)) return EGL_NO_DISPLAY;
    return eglGetDisplay(EGL_DEFAULT_DISPLAY);
}
EGLSurface eglCreatePlatformWindowSurface(EGLDisplay display, EGLConfig config, void *window, const EGLAttrib *attributes) {
    EGLint converted[64];
    if (attributes) {
        unsigned i;
        for (i = 0; i < 63 && attributes[i] != EGL_NONE; ++i) {
            if (attributes[i] < INT_MIN || attributes[i] > INT_MAX) return EGL_NO_SURFACE;
            converted[i] = (EGLint)attributes[i];
        }
        if (i == 63) return EGL_NO_SURFACE;
        converted[i] = EGL_NONE;
    }
    return eglCreateWindowSurface(display, config, (EGLNativeWindowType)(intptr_t)window, attributes ? converted : 0);
}

#include <string.h>
#include <webgl/webgl2.h>
extern void *emscripten_GetProcAddress(const char *name);

/* WebGL 2 exposes getBufferSubData, but Emscripten's GLES procedure table omits
 * this desktop-named entry. wgpu uses it for readback after its fence completes. */
__eglMustCastToProperFunctionPointerType eglGetProcAddress(const char *name) {
    if (strcmp(name, "glGetBufferSubData") == 0) {
        return (__eglMustCastToProperFunctionPointerType)glGetBufferSubData;
    }
    return (__eglMustCastToProperFunctionPointerType)emscripten_GetProcAddress(name);
}
