// SPDX-License-Identifier: MIT
// GTK3 bridge for backend dialogs which intentionally disable GTK_USE_PORTAL.
// Only stable opaque GtkSettings/GdkDisplay APIs are used; headers stay optional.
#include <gio/gio.h>
#include <gmodule.h>
extern GObject *gtk_settings_get_default(void);
extern GObject *gtk_settings_get_for_screen(void *screen);
extern void *gdk_display_get_default_screen(void *display);
extern void *gdk_display_get_default(void);
static GDBusProxy *proxy;
static char *font_name, *theme_name;
static void apply(GObject *settings) {
    if (!settings) return;
    if (font_name) g_object_set(settings, "gtk-font-name", font_name, NULL);
    if (theme_name) g_object_set(settings, "gtk-theme-name", theme_name, NULL);
}
static void setting(const char *space, const char *key, GVariant *value) {
    if (g_strcmp0(space, "org.gnome.desktop.interface") || !g_variant_is_of_type(value, G_VARIANT_TYPE_STRING)) return;
    char **target = !g_strcmp0(key,"font-name") ? &font_name : !g_strcmp0(key,"gtk-theme") ? &theme_name : NULL;
    if (!target) return;
    g_free(*target); *target = g_variant_dup_string(value, NULL);
}
static void changed(GDBusProxy *object, const char *sender, const char *signal, GVariant *parameters, void *data) {
    (void)object; (void)sender; (void)data;
    if (g_strcmp0(signal,"SettingChanged")) return;
    const char *space, *key; GVariant *value;
    g_variant_get(parameters,"(&s&sv)",&space,&key,&value);
    setting(space,key,value); g_variant_unref(value);
    if (gdk_display_get_default()) apply(gtk_settings_get_default());
}
G_MODULE_EXPORT void gtk_module_init(int *argc, char ***argv) {
    (void)argc; (void)argv;
    if (proxy) return;
    GError *error = NULL;
    proxy = g_dbus_proxy_new_for_bus_sync(G_BUS_TYPE_SESSION, G_DBUS_PROXY_FLAGS_DO_NOT_AUTO_START,
        NULL,"org.freedesktop.impl.portal.desktop.dome","/org/freedesktop/portal/desktop",
        "org.freedesktop.impl.portal.Settings",NULL,&error);
    if (!proxy) { g_clear_error(&error); return; }
    const char *spaces[]={"org.gnome.desktop.interface",NULL};
    GVariant *reply = g_dbus_proxy_call_sync(proxy,"ReadAll",g_variant_new("(^as)",spaces),
        G_DBUS_CALL_FLAGS_NONE,2000,NULL,&error);
    if (reply) {
        GVariant *all = g_variant_get_child_value(reply,0);
        GVariant *values = g_variant_lookup_value(all,"org.gnome.desktop.interface",G_VARIANT_TYPE("a{sv}"));
        if (values) {
            GVariantIter iter;const char *key;GVariant *value;g_variant_iter_init(&iter,values);
            while (g_variant_iter_next(&iter,"{&sv}",&key,&value)) {
                setting("org.gnome.desktop.interface",key,value);g_variant_unref(value);
            }
            g_variant_unref(values);
        }
        g_variant_unref(all);g_variant_unref(reply);
    }
    g_clear_error(&error);
    g_signal_connect(proxy,"g-signal",G_CALLBACK(changed),NULL);
    if (gdk_display_get_default()) apply(gtk_settings_get_default());
}
G_MODULE_EXPORT void gtk_module_display_init(void *display) {
    if (!proxy) gtk_module_init(NULL,NULL);
    apply(gtk_settings_get_for_screen(gdk_display_get_default_screen(display)));
}
