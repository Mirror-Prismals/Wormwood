// Context-borrowing GLES3 vegetation material and silhouette shadow passes.
#import <Foundation/Foundation.h>
#import <OpenGLES/EAGL.h>
#import <OpenGLES/ES3/gl.h>
#include "GLES.h"
#include <limits.h>
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    GLint view, sun_view, sun_direction, sun_color, ambient, shadows, depth;
} Uniforms;

typedef struct {
    GLint viewport[4];
    GLfloat depth_range[2], clear_depth;
    GLboolean scissor, blend, cull, offset, discard, stencil, sample_coverage, alpha_coverage;
} ParentState;

static ParentState parent_state(void) {
    ParentState state;
    glGetIntegerv(GL_VIEWPORT, state.viewport);
    glGetFloatv(GL_DEPTH_RANGE, state.depth_range);
    glGetFloatv(GL_DEPTH_CLEAR_VALUE, &state.clear_depth);
    state.scissor = glIsEnabled(GL_SCISSOR_TEST);
    state.blend = glIsEnabled(GL_BLEND);
    state.cull = glIsEnabled(GL_CULL_FACE);
    state.offset = glIsEnabled(GL_POLYGON_OFFSET_FILL);
    state.discard = glIsEnabled(GL_RASTERIZER_DISCARD);
    state.stencil = glIsEnabled(GL_STENCIL_TEST);
    state.sample_coverage = glIsEnabled(GL_SAMPLE_COVERAGE);
    state.alpha_coverage = glIsEnabled(GL_SAMPLE_ALPHA_TO_COVERAGE);
    return state;
}

static void enabled(GLenum flag, GLboolean value) {
    if (value) glEnable(flag); else glDisable(flag);
}

struct WwVegetationGLES {
    void *context; // Borrowed: parent keeps the context alive through destroy.
    GLuint program, shadow_program, vao, vbo, ebo, depth, shadow_fbo, sampler;
    uint32_t shadow_size;
    Uniforms color_uniforms;
    GLint shadow_view;
    int shadow_valid;
    float *shadow_vertices;
    uint32_t *shadow_indices;
    size_t shadow_vertex_count, shadow_index_count;
    float shadow_sun_view[16];
};

static int fail(char *error, size_t capacity, const char *format, ...) {
    if (error && capacity) {
        va_list args;
        va_start(args, format);
        vsnprintf(error, capacity, format, args);
        va_end(args);
    }
    return -1;
}

static int current(const WwVegetationGLES *client) {
    return client && client->context == (__bridge void *)[EAGLContext currentContext];
}

static int checked(char *error, size_t capacity, const char *operation) {
    GLenum code = glGetError();
    if (code != GL_NO_ERROR)
        return fail(error, capacity, "vegetation GLES %s: 0x%x", operation, code);
    return 0;
}

static int valid_input(const float *vertices, size_t vertex_count,
                       const uint32_t *indices, size_t index_count,
                       const WwVegetationScene *scene, uint32_t width,
                       uint32_t height, char *error, size_t capacity) {
    if (!scene || !width || !height || width > INT_MAX || height > INT_MAX ||
        vertex_count > 98304 || index_count > 98304 || index_count % 3 ||
        (vertex_count && !vertices) || (index_count && !indices))
        return fail(error, capacity, "invalid vegetation scene/buffer dimensions");
    for (size_t i = 0; i < vertex_count * 16; ++i)
        if (!isfinite(vertices[i])) return fail(error, capacity, "nonfinite vegetation vertex");
    for (size_t i = 0; i < vertex_count; ++i) {
        float material = vertices[i * 16 + 11];
        if (material != 0.f && material != 1.f && material != 2.f)
            return fail(error, capacity, "invalid vegetation material");
    }
    for (size_t i = 0; i < index_count; ++i)
        if (indices[i] >= vertex_count) return fail(error, capacity, "vegetation index out of bounds");
    for (size_t i = 0; i < 16; ++i)
        if (!isfinite(scene->view_projection[i]) || !isfinite(scene->sun_view_projection[i]))
            return fail(error, capacity, "nonfinite vegetation matrix");
    double light_squared = 0.;
    for (size_t i = 0; i < 3; ++i) {
        if (!isfinite(scene->sun_direction[i]) || !isfinite(scene->sun_color[i]) ||
            !isfinite(scene->ambient_color[i]))
            return fail(error, capacity, "nonfinite vegetation lighting");
        light_squared += (double)scene->sun_direction[i] * scene->sun_direction[i];
    }
    if (light_squared < 1e-12 || (scene->shadows_enabled != 0 && scene->shadows_enabled != 1))
        return fail(error, capacity, "invalid vegetation sun/shadow selection");
    return 0;
}

static GLuint shader(NSString *root, NSString *name, GLenum kind, char *error, size_t capacity) {
    NSString *path = [[root stringByAppendingPathComponent:@"Procedures/Shaders"] stringByAppendingPathComponent:name];
    NSError *read_error = nil;
    NSString *source = [NSString stringWithContentsOfFile:path encoding:NSUTF8StringEncoding error:&read_error];
    if (!source || [source lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > INT_MAX) {
        fail(error, capacity, "vegetation shader %s: %s", name.UTF8String,
             source ? "source too large" : read_error.localizedDescription.UTF8String);
        return 0;
    }
    const GLchar *text = source.UTF8String;
    GLint length = (GLint)[source lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
    GLuint handle = glCreateShader(kind);
    if (!handle) { fail(error, capacity, "cannot create vegetation shader"); return 0; }
    glShaderSource(handle, 1, &text, &length);
    glCompileShader(handle);
    GLint compiled = 0;
    glGetShaderiv(handle, GL_COMPILE_STATUS, &compiled);
    if (!compiled) {
        GLchar log[2048] = {0};
        glGetShaderInfoLog(handle, sizeof(log), NULL, log);
        fail(error, capacity, "vegetation shader %s: %s", name.UTF8String, log);
        glDeleteShader(handle);
        return 0;
    }
    return handle;
}

static GLuint program(NSString *root, NSString *fragment, char *error, size_t capacity) {
    GLuint vertex = shader(root, @"vegetation.ios.vert", GL_VERTEX_SHADER, error, capacity);
    if (!vertex) return 0;
    GLuint pixel = shader(root, fragment, GL_FRAGMENT_SHADER, error, capacity);
    if (!pixel) { glDeleteShader(vertex); return 0; }
    GLuint handle = glCreateProgram();
    if (handle) {
        glAttachShader(handle, vertex); glAttachShader(handle, pixel); glLinkProgram(handle);
    }
    glDeleteShader(vertex); glDeleteShader(pixel);
    GLint linked = 0;
    if (handle) glGetProgramiv(handle, GL_LINK_STATUS, &linked);
    if (!linked) {
        GLchar log[2048] = {0};
        if (handle) glGetProgramInfoLog(handle, sizeof(log), NULL, log);
        fail(error, capacity, "vegetation link %s: %s", fragment.UTF8String, log);
        if (handle) glDeleteProgram(handle);
        return 0;
    }
    return handle;
}

void ww_vegetation_gles_destroy(WwVegetationGLES *client) {
    if (!client || !current(client)) return; // Destruction requires its owning context.
    if (client->program) glDeleteProgram(client->program);
    if (client->shadow_program) glDeleteProgram(client->shadow_program);
    if (client->vao) glDeleteVertexArrays(1, &client->vao);
    if (client->vbo) glDeleteBuffers(1, &client->vbo);
    if (client->ebo) glDeleteBuffers(1, &client->ebo);
    if (client->depth) glDeleteTextures(1, &client->depth);
    if (client->shadow_fbo) glDeleteFramebuffers(1, &client->shadow_fbo);
    if (client->sampler) glDeleteSamplers(1, &client->sampler);
    free(client->shadow_vertices); free(client->shadow_indices); free(client);
}

WwVegetationGLES *ww_vegetation_gles_create(const char *shader_root, char *error, size_t capacity) {
    @autoreleasepool {
        if (error && capacity) error[0] = 0;
        EAGLContext *context = [EAGLContext currentContext];
        if (!context || context.API != kEAGLRenderingAPIOpenGLES3 || !shader_root) {
            fail(error, capacity, "vegetation requires current GLES3 context and asset root"); return NULL;
        }
        if (checked(error, capacity, "entry state")) return NULL;
        NSString *root = [NSString stringWithUTF8String:shader_root];
        if (!root) { fail(error, capacity, "invalid vegetation asset root"); return NULL; }
        WwVegetationGLES *client = calloc(1, sizeof(*client));
        if (!client) { fail(error, capacity, "vegetation client allocation failed"); return NULL; }
        client->context = (__bridge void *)context;
        client->program = program(root, @"vegetation.ios.frag", error, capacity);
        if (!client->program) { ww_vegetation_gles_destroy(client); return NULL; }
        client->shadow_program = program(root, @"vegetation.shadow.ios.frag", error, capacity);
        if (!client->shadow_program) { ww_vegetation_gles_destroy(client); return NULL; }
        Uniforms *u = &client->color_uniforms;
        u->view = glGetUniformLocation(client->program, "viewProjection");
        u->sun_view = glGetUniformLocation(client->program, "sunViewProjection");
        u->sun_direction = glGetUniformLocation(client->program, "sunDirection");
        u->sun_color = glGetUniformLocation(client->program, "sunColor");
        u->ambient = glGetUniformLocation(client->program, "ambientColor");
        u->shadows = glGetUniformLocation(client->program, "shadowsEnabled");
        u->depth = glGetUniformLocation(client->program, "shadowDepth");
        client->shadow_view = glGetUniformLocation(client->shadow_program, "viewProjection");
        if (u->view < 0 || u->sun_view < 0 || u->sun_direction < 0 || u->sun_color < 0 ||
            u->ambient < 0 || u->shadows < 0 || u->depth < 0 || client->shadow_view < 0) {
            fail(error, capacity, "vegetation shader uniform ABI mismatch");
            ww_vegetation_gles_destroy(client); return NULL;
        }
        GLint old_vao = 0, old_buffer = 0;
        glGetIntegerv(GL_VERTEX_ARRAY_BINDING, &old_vao);
        glGetIntegerv(GL_ARRAY_BUFFER_BINDING, &old_buffer);
        glGenVertexArrays(1, &client->vao); glBindVertexArray(client->vao);
        glGenBuffers(1, &client->vbo); glBindBuffer(GL_ARRAY_BUFFER, client->vbo);
        glGenBuffers(1, &client->ebo); glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, client->ebo);
        const GLint sizes[6] = {3, 3, 2, 3, 4, 1};
        const uintptr_t offsets[6] = {0, 12, 24, 32, 44, 60};
        for (GLuint i = 0; i < 6; ++i) {
            glEnableVertexAttribArray(i);
            glVertexAttribPointer(i, sizes[i], GL_FLOAT, GL_FALSE, 64, (const void *)offsets[i]);
        }
        glBindVertexArray((GLuint)old_vao); glBindBuffer(GL_ARRAY_BUFFER, (GLuint)old_buffer);
        glGenSamplers(1, &client->sampler);
        glSamplerParameteri(client->sampler, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        glSamplerParameteri(client->sampler, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        glSamplerParameteri(client->sampler, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        glSamplerParameteri(client->sampler, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
        glSamplerParameteri(client->sampler, GL_TEXTURE_COMPARE_MODE, GL_NONE);
        if (checked(error, capacity, "create buffers/sampler") || !client->vao ||
            !client->vbo || !client->ebo || !client->sampler) {
            if (error && capacity && !error[0]) fail(error, capacity, "vegetation buffer creation failed");
            ww_vegetation_gles_destroy(client); return NULL;
        }
        return client;
    }
}

static void restore(GLuint framebuffer, uint32_t width, uint32_t height, const ParentState *previous) {
    glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
    if (width && height && width <= INT_MAX && height <= INT_MAX) glViewport(0, 0, (GLsizei)width, (GLsizei)height);
    else glViewport(previous->viewport[0], previous->viewport[1], previous->viewport[2], previous->viewport[3]);
    enabled(GL_SCISSOR_TEST, previous->scissor); enabled(GL_BLEND, previous->blend);
    enabled(GL_CULL_FACE, previous->cull); enabled(GL_POLYGON_OFFSET_FILL, previous->offset);
    enabled(GL_RASTERIZER_DISCARD, previous->discard); enabled(GL_STENCIL_TEST, previous->stencil);
    enabled(GL_SAMPLE_COVERAGE, previous->sample_coverage); enabled(GL_SAMPLE_ALPHA_TO_COVERAGE, previous->alpha_coverage);
    glDepthRangef(previous->depth_range[0], previous->depth_range[1]); glClearDepthf(previous->clear_depth);
    glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);
    glEnable(GL_DEPTH_TEST); glDepthMask(GL_TRUE); glDepthFunc(GL_LESS);
}

static void upload(WwVegetationGLES *client, const float *vertices, size_t vertex_count,
                   const uint32_t *indices, size_t index_count) {
    glBindVertexArray(client->vao);
    glBindBuffer(GL_ARRAY_BUFFER, client->vbo);
    glBufferData(GL_ARRAY_BUFFER, vertex_count * 64, vertices, GL_DYNAMIC_DRAW);
    glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, client->ebo);
    glBufferData(GL_ELEMENT_ARRAY_BUFFER, index_count * sizeof(uint32_t), indices, GL_DYNAMIC_DRAW);
}

int ww_vegetation_gles_shadow(WwVegetationGLES *client, const float *vertices, size_t vertex_count,
                              const uint32_t *indices, size_t index_count, const WwVegetationScene *scene,
                              uint32_t shadow_size, uint32_t restore_framebuffer, uint32_t width,
                              uint32_t height, char *error, size_t capacity) {
    if (error && capacity) error[0] = 0;
    if (!current(client)) return fail(error, capacity, "vegetation shadow requires owning current context");
    ParentState previous = parent_state();
    int result = -1;
    float *copy_vertices = NULL; uint32_t *copy_indices = NULL;
    client->shadow_valid = 0;
    if (valid_input(vertices, vertex_count, indices, index_count, scene, width, height, error, capacity)) goto done;
    if (checked(error, capacity, "shadow entry")) goto done;
    GLint maximum = 0; glGetIntegerv(GL_MAX_TEXTURE_SIZE, &maximum);
    if (!shadow_size || shadow_size > (uint32_t)maximum) {
        fail(error, capacity, "vegetation shadow texture exceeds device dimensions"); goto done;
    }
    if (vertex_count) copy_vertices = malloc(vertex_count * 64);
    if (index_count) copy_indices = malloc(index_count * sizeof(uint32_t));
    if ((vertex_count && !copy_vertices) || (index_count && !copy_indices)) {
        fail(error, capacity, "vegetation shadow packet allocation failed"); goto done;
    }
    if (vertex_count) memcpy(copy_vertices, vertices, vertex_count * 64);
    if (index_count) memcpy(copy_indices, indices, index_count * sizeof(uint32_t));
    glActiveTexture(GL_TEXTURE4);
    if (!client->depth) glGenTextures(1, &client->depth);
    if (!client->shadow_fbo) glGenFramebuffers(1, &client->shadow_fbo);
    if (!client->depth || !client->shadow_fbo) {
        fail(error, capacity, "vegetation sunlight resource creation failed"); goto done;
    }
    glBindTexture(GL_TEXTURE_2D, client->depth);
    if (client->shadow_size != shadow_size) {
        glTexImage2D(GL_TEXTURE_2D, 0, GL_DEPTH_COMPONENT24, shadow_size, shadow_size, 0,
                     GL_DEPTH_COMPONENT, GL_UNSIGNED_INT, NULL);
        if (checked(error, capacity, "allocate sunlight depth texture")) goto done;
        client->shadow_size = shadow_size;
    }
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_COMPARE_MODE, GL_NONE);
    glBindSampler(4, client->sampler);
    glBindFramebuffer(GL_FRAMEBUFFER, client->shadow_fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_DEPTH_ATTACHMENT, GL_TEXTURE_2D, client->depth, 0);
    const GLenum none = GL_NONE; glDrawBuffers(1, &none); glReadBuffer(GL_NONE);
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) {
        fail(error, capacity, "vegetation sunlight framebuffer incomplete"); goto done;
    }
    glViewport(0, 0, shadow_size, shadow_size);
    glDisable(GL_SCISSOR_TEST); glDisable(GL_BLEND); glDisable(GL_CULL_FACE);
    glDisable(GL_POLYGON_OFFSET_FILL); glDisable(GL_RASTERIZER_DISCARD);
    glDisable(GL_STENCIL_TEST); glDisable(GL_SAMPLE_COVERAGE); glDisable(GL_SAMPLE_ALPHA_TO_COVERAGE);
    glDepthRangef(0.f, 1.f); glEnable(GL_DEPTH_TEST); glDepthFunc(GL_LESS); glDepthMask(GL_TRUE);
    glColorMask(GL_FALSE, GL_FALSE, GL_FALSE, GL_FALSE);
    glClearDepthf(1.f); glClear(GL_DEPTH_BUFFER_BIT);
    glUseProgram(client->shadow_program);
    glUniformMatrix4fv(client->shadow_view, 1, GL_FALSE, scene->sun_view_projection);
    upload(client, vertices, vertex_count, indices, index_count);
    if (index_count) glDrawElements(GL_TRIANGLES, (GLsizei)index_count, GL_UNSIGNED_INT, NULL);
    if (checked(error, capacity, "sunlight depth draw")) goto done;
    free(client->shadow_vertices); free(client->shadow_indices);
    client->shadow_vertices = copy_vertices; copy_vertices = NULL;
    client->shadow_indices = copy_indices; copy_indices = NULL;
    client->shadow_vertex_count = vertex_count; client->shadow_index_count = index_count;
    memcpy(client->shadow_sun_view, scene->sun_view_projection, sizeof(client->shadow_sun_view));
    client->shadow_valid = 1;
    result = 0;
done:
    free(copy_vertices); free(copy_indices);
    restore(restore_framebuffer, width, height, &previous);
    if (result == 0) result = checked(error, capacity, "restore after sunlight pass");
    if (result != 0) client->shadow_valid = 0;
    return result;
}

uint32_t ww_vegetation_gles_shadow_texture(const WwVegetationGLES *client) {
    return client && client->shadow_valid ? client->depth : 0;
}

int ww_vegetation_gles_draw(WwVegetationGLES *client, const float *vertices, size_t vertex_count,
                            const uint32_t *indices, size_t index_count, const WwVegetationScene *scene,
                            uint32_t framebuffer, uint32_t width, uint32_t height, char *error, size_t capacity) {
    if (error && capacity) error[0] = 0;
    if (!current(client)) return fail(error, capacity, "vegetation draw requires owning current context");
    ParentState previous = parent_state();
    int result = -1;
    if (valid_input(vertices, vertex_count, indices, index_count, scene, width, height, error, capacity)) goto done;
    if (checked(error, capacity, "draw entry")) goto done;
    if (scene->shadows_enabled && (!client->shadow_valid || vertex_count != client->shadow_vertex_count ||
        index_count != client->shadow_index_count ||
        (vertex_count && memcmp(vertices, client->shadow_vertices, vertex_count * 64)) ||
        (index_count && memcmp(indices, client->shadow_indices, index_count * sizeof(uint32_t))) ||
        memcmp(scene->sun_view_projection, client->shadow_sun_view, sizeof(client->shadow_sun_view)))) {
        fail(error, capacity, "vegetation shadow packet/matrix is absent or stale"); goto done;
    }
    if (framebuffer == client->shadow_fbo && client->shadow_fbo) {
        fail(error, capacity, "vegetation color target cannot be its sunlight target"); goto done;
    }
    glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) {
        fail(error, capacity, "vegetation opaque target incomplete"); goto done;
    }
    glViewport(0, 0, width, height);
    glDisable(GL_SCISSOR_TEST); glDisable(GL_BLEND); glDisable(GL_CULL_FACE);
    glDisable(GL_POLYGON_OFFSET_FILL); glDisable(GL_RASTERIZER_DISCARD);
    glDisable(GL_STENCIL_TEST); glDisable(GL_SAMPLE_COVERAGE); glDisable(GL_SAMPLE_ALPHA_TO_COVERAGE);
    glDepthRangef(0.f, 1.f); glEnable(GL_DEPTH_TEST); glDepthMask(GL_TRUE); glDepthFunc(GL_LESS);
    glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);
    glUseProgram(client->program);
    const Uniforms *u = &client->color_uniforms;
    glUniformMatrix4fv(u->view, 1, GL_FALSE, scene->view_projection);
    glUniformMatrix4fv(u->sun_view, 1, GL_FALSE, scene->sun_view_projection);
    glUniform3fv(u->sun_direction, 1, scene->sun_direction);
    glUniform3fv(u->sun_color, 1, scene->sun_color);
    glUniform3fv(u->ambient, 1, scene->ambient_color);
    glUniform1i(u->shadows, scene->shadows_enabled); glUniform1i(u->depth, 4);
    glActiveTexture(GL_TEXTURE4); glBindTexture(GL_TEXTURE_2D, client->depth);
    glBindSampler(4, client->sampler);
    upload(client, vertices, vertex_count, indices, index_count);
    if (index_count) glDrawElements(GL_TRIANGLES, (GLsizei)index_count, GL_UNSIGNED_INT, NULL);
    result = checked(error, capacity, "indexed color draw");
done:
    restore(framebuffer, width, height, &previous);
    if (result == 0) result = checked(error, capacity, "restore after color pass");
    return result;
}
