// SPDX-License-Identifier: MIT
#pragma once
#include <gio/gio.h>
#include <string>
#include <unistd.h>
namespace login_session {
// User-manager services can live in a non-graphical "manager" session. Resolve
// the user's display explicitly, instead of registering an authentication agent
// or watching idle state for that unrelated manager session.
inline std::string id() {
  if (auto value = getenv("XDG_SESSION_ID"); value && *value)
    return value;
  std::string result;
  auto system = g_bus_get_sync(G_BUS_TYPE_SYSTEM, nullptr, nullptr);
  if (!system)
    return result;
  auto user = g_dbus_connection_call_sync(
      system, "org.freedesktop.login1", "/org/freedesktop/login1",
      "org.freedesktop.login1.Manager", "GetUser",
      g_variant_new("(u)", guint(getuid())), G_VARIANT_TYPE("(o)"),
      G_DBUS_CALL_FLAGS_NONE, 3000, nullptr, nullptr);
  if (user) {
    const char *path;
    g_variant_get(user, "(&o)", &path);
    auto display = g_dbus_connection_call_sync(
        system, "org.freedesktop.login1", path,
        "org.freedesktop.DBus.Properties", "Get",
        g_variant_new("(ss)", "org.freedesktop.login1.User", "Display"),
        G_VARIANT_TYPE("(v)"), G_DBUS_CALL_FLAGS_NONE, 3000, nullptr, nullptr);
    if (display) {
      GVariant *value;
      g_variant_get(display, "(v)", &value);
      const char *session, *object;
      g_variant_get(value, "(&s&o)", &session, &object);
      result = session;
      g_variant_unref(value);
      g_variant_unref(display);
    }
    g_variant_unref(user);
  }
  g_object_unref(system);
  return result;
}
} // namespace login_session
