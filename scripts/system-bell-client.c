/* Native client fixture for the standard xdg-system-bell protocol. */
#include <string.h>
#include <wayland-client.h>
#include "xdg-system-bell-v1-client-protocol.h"
static struct xdg_system_bell_v1 *bell;
static void global(void *data,struct wl_registry *registry,uint32_t id,const char *interface,uint32_t version) {
    (void)data;(void)version;
    if (!strcmp(interface,"xdg_system_bell_v1")) bell=wl_registry_bind(registry,id,&xdg_system_bell_v1_interface,1);
}
static void removed(void *data,struct wl_registry *registry,uint32_t id) { (void)data;(void)registry;(void)id; }
int main(void) {
    struct wl_display *display=wl_display_connect(NULL);if (!display) return 1;
    struct wl_registry *registry=wl_display_get_registry(display);
    static const struct wl_registry_listener listener={global,removed};wl_registry_add_listener(registry,&listener,NULL);
    if (wl_display_roundtrip(display)<0 || !bell) return 2;
    xdg_system_bell_v1_ring(bell,NULL);if (wl_display_roundtrip(display)<0) return 3;
    xdg_system_bell_v1_destroy(bell);wl_registry_destroy(registry);wl_display_flush(display);wl_display_disconnect(display);return 0;
}
