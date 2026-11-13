#ifndef MIRROR_VEGETATION_GLES_H
#define MIRROR_VEGETATION_GLES_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Parent owns the current GLES3 context, drawable and opaque scene pass.
 * Helper owns only indexed plant buffers/programs and a sampleable sunlight
 * depth texture/FBO. Create, encode and destroy require that current context.
 * Vertices: 16 floats / 64 bytes; attributes 0..5 = 3/3/2/3/4/1 floats at
 * offsets 0/12/24/32/44/60. Indices: uint32_t, CCW, two-sided foliage.
 * All matrices are column-major, canonical 0..1 clip depth and camera-relative
 * to the same packet origin. ES shader converts clip depth to OpenGL depth.
 * These are standalone shader uniforms, not a Scene UBO memory layout. */
typedef struct {
    float view_projection[16];
    float sun_view_projection[16];
    float sun_direction[3];
    float sun_color[3];
    float ambient_color[3];
    int shadows_enabled;
} WwVegetationScene;
typedef struct WwVegetationGLES WwVegetationGLES;
WwVegetationGLES *ww_vegetation_gles_create(const char *shader_root,
    char *error,size_t capacity);
/* Encode sunlight depth with the existing alpha silhouette shader. Depth
 * clear 1, LESS test, compare mode NONE, clamp-to-edge sampling. Return to
 * restore_framebuffer/viewport even on failure. No window or presentation. */
int ww_vegetation_gles_shadow(WwVegetationGLES *client,
    const float *vertices,size_t vertex_count,
    const uint32_t *indices,size_t index_count,
    const WwVegetationScene *scene,uint32_t shadow_size,
    uint32_t restore_framebuffer,uint32_t width,uint32_t height,
    char *error,size_t capacity);
uint32_t ww_vegetation_gles_shadow_texture(const WwVegetationGLES *client);
/* Draw into an already rendered opaque framebuffer without clearing it.
 * Scene and sunlight pass must use identical origin/geometry revision.
 * Shadow texture uses unit 4. Parent rebinds its own program/VAO/textures
 * after this call. Keep depth test/write enabled, LESS, color mask enabled. */
int ww_vegetation_gles_draw(WwVegetationGLES *client,
    const float *vertices,size_t vertex_count,
    const uint32_t *indices,size_t index_count,
    const WwVegetationScene *scene,
    uint32_t framebuffer,uint32_t width,uint32_t height,
    char *error,size_t capacity);
void ww_vegetation_gles_destroy(WwVegetationGLES *client);
#ifdef __cplusplus
}
#endif
#endif
