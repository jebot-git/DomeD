/* SPDX-License-Identifier: MIT
 * A bounded CPU RGBA source. All PipeWire callbacks run under the thread-loop
 * lock; the compositor copies a frame under that same lock, never in RT code.
 */
#include <pipewire/pipewire.h>
#include <spa/param/video/format-utils.h>
#include <spa/buffer/meta.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <pthread.h>
static pthread_once_t initialized = PTHREAD_ONCE_INIT;
static void initialize(void) { pw_init(NULL, NULL); }

struct dome_cast {
    struct pw_thread_loop *loop;
    struct pw_stream *stream;
    uint8_t *pixels;
    uint32_t width, height, node, sequence;
    int failed;
};
static void state_changed(void *p, enum pw_stream_state old,
                          enum pw_stream_state state, const char *error) {
    (void)old; (void)error;
    struct dome_cast *s = p;
    if (state == PW_STREAM_STATE_ERROR || state == PW_STREAM_STATE_UNCONNECTED)
        s->failed = 1;
    if (state == PW_STREAM_STATE_PAUSED || state == PW_STREAM_STATE_STREAMING)
        s->node = pw_stream_get_node_id(s->stream);
    pw_thread_loop_signal(s->loop, false);
}
static void process(void *p) {
    struct dome_cast *s = p;
    struct pw_buffer *b = pw_stream_dequeue_buffer(s->stream);
    if (!b) return;
    struct spa_buffer *buf = b->buffer;
    const uint32_t stride = s->width * 4, size = stride * s->height;
    if (buf->n_datas && buf->datas[0].data && buf->datas[0].maxsize >= size) {
        memcpy(buf->datas[0].data, s->pixels, size);
        buf->datas[0].chunk->offset = 0;
        buf->datas[0].chunk->stride = stride;
        buf->datas[0].chunk->size = size;
        struct spa_meta_header *h = spa_buffer_find_meta_data(buf, SPA_META_Header, sizeof(*h));
        if (h) {
            struct timespec now;
            clock_gettime(CLOCK_MONOTONIC, &now);
            h->pts = (int64_t)now.tv_sec * SPA_NSEC_PER_SEC + now.tv_nsec;
            h->flags = 0; h->seq = s->sequence++; h->dts_offset = 0;
        }
    } else if (buf->n_datas && buf->datas[0].chunk) {
        buf->datas[0].chunk->size = 0;
    }
    pw_stream_queue_buffer(s->stream, b);
}
static void format_changed(void *p, uint32_t id, const struct spa_pod *param) {
    struct dome_cast *s = p;
    if (id != SPA_PARAM_Format || !param) return;
    uint8_t buffer[512];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(buffer, sizeof(buffer));
    const struct spa_pod *params[2];
    params[0] = spa_pod_builder_add_object(&b,
        SPA_TYPE_OBJECT_ParamBuffers, SPA_PARAM_Buffers,
        SPA_PARAM_BUFFERS_buffers, SPA_POD_CHOICE_RANGE_Int(4, 2, 8),
        SPA_PARAM_BUFFERS_blocks, SPA_POD_Int(1),
        SPA_PARAM_BUFFERS_size, SPA_POD_Int(s->width * s->height * 4),
        SPA_PARAM_BUFFERS_stride, SPA_POD_Int(s->width * 4),
        SPA_PARAM_BUFFERS_dataType, SPA_POD_CHOICE_FLAGS_Int(1 << SPA_DATA_MemFd));
    params[1] = spa_pod_builder_add_object(&b,
        SPA_TYPE_OBJECT_ParamMeta, SPA_PARAM_Meta,
        SPA_PARAM_META_type, SPA_POD_Id(SPA_META_Header),
        SPA_PARAM_META_size, SPA_POD_Int(sizeof(struct spa_meta_header)));
    pw_stream_update_params(s->stream, params, 2);
}
static const struct pw_stream_events events = {
    PW_VERSION_STREAM_EVENTS,
    .state_changed = state_changed, .param_changed = format_changed, .process = process,
};
void dome_cast_destroy(struct dome_cast *s) {
    if (!s) return;
    if (s->loop) {
        pw_thread_loop_stop(s->loop);
        if (s->stream) pw_stream_destroy(s->stream);
        pw_thread_loop_destroy(s->loop);
    }
    free(s->pixels); free(s);
}
struct dome_cast *dome_cast_create(const char *name, uint32_t width, uint32_t height) {
    if (!width || !height || width > 16384 || height > 16384 ||
        (uint64_t)width * height * 4 > 256 * 1024 * 1024) return NULL;
    pthread_once(&initialized, initialize);
    struct dome_cast *s = calloc(1, sizeof(*s));
    if (!s) return NULL;
    s->width = width; s->height = height; s->node = PW_ID_ANY;
    s->pixels = calloc((size_t)width * height, 4);
    s->loop = pw_thread_loop_new("dome-screencast", NULL);
    if (!s->pixels || !s->loop) goto fail;
    s->stream = pw_stream_new_simple(pw_thread_loop_get_loop(s->loop), "DomeD ScreenCast",
        pw_properties_new(PW_KEY_MEDIA_TYPE, "Video", PW_KEY_MEDIA_CATEGORY, "Capture",
            PW_KEY_MEDIA_ROLE, "Screen", PW_KEY_MEDIA_CLASS, "Stream/Output/Video",
            PW_KEY_NODE_NAME, name, PW_KEY_NODE_DESCRIPTION, name,
            PW_KEY_NODE_VIRTUAL, "true", NULL), &events, s);
    if (!s->stream) goto fail;
    uint8_t buffer[512];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(buffer, sizeof(buffer));
    const struct spa_pod *format = spa_pod_builder_add_object(&b,
        SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat,
        SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video),
        SPA_FORMAT_mediaSubtype, SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
        SPA_FORMAT_VIDEO_format, SPA_POD_Id(SPA_VIDEO_FORMAT_RGBA),
        SPA_FORMAT_VIDEO_size, SPA_POD_Rectangle(&SPA_RECTANGLE(width, height)),
        SPA_FORMAT_VIDEO_framerate, SPA_POD_Fraction(&SPA_FRACTION(30, 1)));
    if (pw_stream_connect(s->stream, PW_DIRECTION_OUTPUT, PW_ID_ANY,
            PW_STREAM_FLAG_MAP_BUFFERS | PW_STREAM_FLAG_DRIVER,
            &format, 1) < 0) goto fail;
    if (pw_thread_loop_start(s->loop) < 0) goto fail;
    pw_thread_loop_lock(s->loop);
    struct timespec deadline;
    pw_thread_loop_get_time(s->loop, &deadline, 5 * SPA_NSEC_PER_SEC);
    while (s->node == PW_ID_ANY && !s->failed) {
        if (pw_thread_loop_timed_wait_full(s->loop, &deadline) < 0) { s->failed = 1; break; }
    }
    pw_thread_loop_unlock(s->loop);
    if (s->failed) goto fail;
    return s;
fail:
    dome_cast_destroy(s); return NULL;
}
uint32_t dome_cast_node(struct dome_cast *s) { return s->node; }
int dome_cast_frame(struct dome_cast *s, const uint8_t *bytes, size_t length, int flipped) {
    const size_t stride = (size_t)s->width * 4;
    if (length != stride * s->height) return -1;
    pw_thread_loop_lock(s->loop);
    if (!s->failed) {
        for (uint32_t y = 0; y < s->height; ++y)
            memcpy(s->pixels + y * stride,
                bytes + (flipped ? s->height - 1 - y : y) * stride, stride);
        pw_stream_trigger_process(s->stream);
    }
    int failed = s->failed;
    pw_thread_loop_unlock(s->loop);
    return failed ? -1 : 0;
}
