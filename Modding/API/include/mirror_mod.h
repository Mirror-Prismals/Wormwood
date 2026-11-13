#ifndef MIRROR_MOD_H
#define MIRROR_MOD_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define MIRROR_MOD_ABI_VERSION 1u
#if defined(_WIN32)
#define MIRROR_MOD_EXPORT __declspec(dllexport)
#else
#define MIRROR_MOD_EXPORT __attribute__((visibility("default")))
#endif
typedef struct { const uint8_t *data; size_t len; } MirrorSlice;
typedef struct { uint8_t *data; size_t len; size_t capacity; } MirrorBuffer;
typedef void (*MirrorFreeBuffer)(MirrorBuffer);
typedef int32_t (*MirrorInvoke)(void *, MirrorSlice, MirrorSlice, MirrorBuffer *);
typedef struct {
 uint32_t abi;
 MirrorInvoke invoke;
 MirrorFreeBuffer free_buffer;
} MirrorHostApi;
typedef struct {
 uint32_t abi;
 /* Optional pure, thread-safe scalar query; input/output each have four doubles. */
 bool (*query)(uint32_t, const double *, double *);
 void *(*create)(MirrorSlice package_root_utf8, MirrorBuffer *error);
 int32_t (*call)(void *, const MirrorHostApi *, void *, MirrorSlice hook_utf8,
                 MirrorSlice input_json_utf8, MirrorBuffer *output);
 void (*destroy)(void *);
 MirrorFreeBuffer free_buffer;
} MirrorModApi;
/* Return a process-lifetime table. Library remains loaded while workers use it. */
MIRROR_MOD_EXPORT const MirrorModApi *mirror_mod_v1(void);
/* Return status 0 for JSON output; nonzero for a UTF-8 error message.
   Release every returned buffer using its producer's free_buffer function.
   No exception may cross this ABI. Host services are valid only during call(). */
#ifdef __cplusplus
}
#endif
#endif
