// Raw Wayland cursor override probe, driven by pointer position and clicks.
#define _GNU_SOURCE
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

static struct wl_compositor *compositor;
static struct wl_shm *shm;
static struct xdg_wm_base *shell;
static struct wl_surface *surface, *cursor;
static struct wl_pointer *pointer;
static struct wl_buffer *background, *magenta, *yellow;
static uint32_t enter_serial;
static int current_mode = -1, revision;

static struct wl_buffer *buffer(int width, int height, uint32_t color) {
    size_t size = width * height * 4;
    int fd = memfd_create("dome-cursor-probe", MFD_CLOEXEC);
    if (fd < 0 || ftruncate(fd, size)) exit(2);
    uint32_t *pixels = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (pixels == MAP_FAILED) exit(2);
    for (int i = 0; i < width * height; i++) pixels[i] = color;
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, size);
    struct wl_buffer *result = wl_shm_pool_create_buffer(
        pool, 0, width, height, width * 4, WL_SHM_FORMAT_ARGB8888);
    wl_shm_pool_destroy(pool);
    munmap(pixels, size);
    close(fd);
    return result;
}

static void request_cursor(wl_fixed_t x) {
    int mode = wl_fixed_to_int(x) / 80;
    if (mode > 3) mode = 3;
    if (mode != current_mode) {
        if (mode == 1) wl_pointer_set_cursor(pointer, enter_serial, NULL, 0, 0);
        else {
            wl_surface_attach(cursor, magenta, 0, 0);
            wl_surface_damage(cursor, 0, 0, 48, 48);
            wl_surface_commit(cursor);
            wl_pointer_set_cursor(pointer, enter_serial, cursor,
                                  mode == 2 ? 24 : 0, mode == 2 ? 24 : 0);
        }
        current_mode = mode;
        printf("mode %d\n", mode);
    } else if (mode == 3) {
        // Update an existing cursor without another set_cursor request.
        wl_surface_attach(cursor, (++revision % 2) ? yellow : magenta, 0, 0);
        wl_surface_damage(cursor, 0, 0, 48, 48);
        wl_surface_commit(cursor);
        printf("commit %d\n", revision);
    }
}
static void enter(void *d, struct wl_pointer *p, uint32_t serial,
                  struct wl_surface *s, wl_fixed_t x, wl_fixed_t y) {
    enter_serial = serial;
    current_mode = -1;
    puts("enter");
    request_cursor(x);
}
static void leave(void *d, struct wl_pointer *p, uint32_t serial, struct wl_surface *s) {
    puts("leave");
}
static void motion(void *d, struct wl_pointer *p, uint32_t time, wl_fixed_t x, wl_fixed_t y) {
    request_cursor(x);
}
static void button(void *d, struct wl_pointer *p, uint32_t serial,
                   uint32_t time, uint32_t code, uint32_t state) {
    if (state == WL_POINTER_BUTTON_STATE_PRESSED) {
        wl_pointer_set_cursor(pointer, enter_serial, NULL, 0, 0);
        puts("button hide");
    }
}
static void axis(void *d, struct wl_pointer *p, uint32_t time, uint32_t axis, wl_fixed_t value) {}
static const struct wl_pointer_listener pointer_events = {
    .enter = enter, .leave = leave, .motion = motion, .button = button, .axis = axis,
};
static void capabilities(void *d, struct wl_seat *seat, uint32_t caps) {
    if ((caps & WL_SEAT_CAPABILITY_POINTER) && !pointer) {
        pointer = wl_seat_get_pointer(seat);
        wl_pointer_add_listener(pointer, &pointer_events, NULL);
    }
}
static const struct wl_seat_listener seat_events = {.capabilities = capabilities};
static void ping(void *d, struct xdg_wm_base *base, uint32_t serial) {
    xdg_wm_base_pong(base, serial);
}
static const struct xdg_wm_base_listener shell_events = {.ping = ping};
static void configure(void *d, struct xdg_surface *xdg, uint32_t serial) {
    xdg_surface_ack_configure(xdg, serial);
    if (!background) {
        background = buffer(320, 200, 0xff408060);
        wl_surface_attach(surface, background, 0, 0);
        wl_surface_damage(surface, 0, 0, 320, 200);
        wl_surface_commit(surface);
    }
}
static const struct xdg_surface_listener configure_events = {.configure = configure};
static void global(void *d, struct wl_registry *r, uint32_t id, const char *iface, uint32_t version) {
    if (!strcmp(iface, "wl_compositor"))
        compositor = wl_registry_bind(r, id, &wl_compositor_interface, 4);
    else if (!strcmp(iface, "wl_shm")) shm = wl_registry_bind(r, id, &wl_shm_interface, 1);
    else if (!strcmp(iface, "wl_seat")) {
        struct wl_seat *seat = wl_registry_bind(r, id, &wl_seat_interface, 1);
        wl_seat_add_listener(seat, &seat_events, NULL);
    } else if (!strcmp(iface, "xdg_wm_base")) {
        shell = wl_registry_bind(r, id, &xdg_wm_base_interface, 1);
        xdg_wm_base_add_listener(shell, &shell_events, NULL);
    }
}
static void removed(void *d, struct wl_registry *r, uint32_t id) {}
static const struct wl_registry_listener registry_events = {.global = global, .global_remove = removed};
int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) return 1;
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_events, NULL);
    wl_display_roundtrip(display);
    if (!compositor || !shm || !shell) return 2;
    cursor = wl_compositor_create_surface(compositor);
    magenta = buffer(48, 48, 0xffff00ff);
    yellow = buffer(48, 48, 0xffffff00);
    surface = wl_compositor_create_surface(compositor);
    struct xdg_surface *xdg = xdg_wm_base_get_xdg_surface(shell, surface);
    xdg_surface_add_listener(xdg, &configure_events, NULL);
    struct xdg_toplevel *top = xdg_surface_get_toplevel(xdg);
    xdg_toplevel_set_app_id(top, "dome-cursor-probe");
    xdg_toplevel_set_title(top, "Cursor probe");
    wl_surface_commit(surface);
    while (wl_display_dispatch(display) >= 0) {}
    return 0;
}
