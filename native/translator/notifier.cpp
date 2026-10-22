// SPDX-License-Identifier: MIT
#include "notifier.h"
#include <cstring>
#include <gio/gio.h>
#include <stdexcept>
#include <vector>
namespace {
constexpr auto name = "org.domed.Translator";
constexpr auto control =
    R"XML(<node><interface name="org.domed.Translator"><method name="Show"><arg type="s" direction="in"/></method></interface></node>)XML";
constexpr auto item = R"XML(<node><interface name="org.kde.StatusNotifierItem">
<property name="Category" type="s" access="read"/><property name="Id" type="s" access="read"/><property name="Title" type="s" access="read"/><property name="Status" type="s" access="read"/><property name="WindowId" type="u" access="read"/><property name="IconName" type="s" access="read"/><property name="IconThemePath" type="s" access="read"/><property name="IconPixmap" type="a(iiay)" access="read"/><property name="ItemIsMenu" type="b" access="read"/><property name="Menu" type="o" access="read"/>
<method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method><method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method><method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method><method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method><signal name="NewIcon"/><signal name="NewStatus"><arg type="s"/></signal></interface></node>)XML";
constexpr auto menu =
    R"XML(<node><interface name="com.canonical.dbusmenu"><property name="Version" type="u" access="read"/><property name="TextDirection" type="s" access="read"/><property name="Status" type="s" access="read"/><property name="IconThemePath" type="as" access="read"/>
<method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/><arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method><method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method><method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/></method><signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal></interface></node>)XML";
} // namespace
struct Notifier::Impl {
  struct Icon {
    Impl *owner;
    std::string path, page, title, icon;
  };
  GDBusConnection *bus = nullptr;
  std::vector<guint> registrations;
  std::vector<GDBusNodeInfo *> infos;
  guint watcher = 0;
  bool selectionEnabled=false;
  guint revision=1;
  std::vector<Icon> icons;
  std::function<void(const std::string &)> show;
  static GVariant *pixmaps() {
    GVariantBuilder images;
    g_variant_builder_init(&images, G_VARIANT_TYPE("a(iiay)"));
    for (int scale : {1, 2}) {
      int size = 24 * scale;
      std::vector<guint8> argb(size * size * 4, 0);
      auto pixel = [&](int x, int y) {
        for (int dy=0; dy<scale; ++dy) for (int dx=0; dx<scale; ++dx) {
          auto at = ((y*scale+dy)*size+x*scale+dx)*4;
          argb[at]=255; argb[at+1]=230; argb[at+2]=235; argb[at+3]=245;
        }
      };
      // Two opposing translation arrows and a clearly legible A.
      for(int x=3; x<=20; ++x) {pixel(x,3); pixel(x,20);}
      for(int i=0; i<4; ++i) {pixel(20-i,3-i/2); pixel(20-i,3+i/2);
        pixel(3+i,20-i/2); pixel(3+i,20+i/2);}
      for(int y=7; y<=16; ++y) {pixel(5+(16-y)/3,y); pixel(11-(16-y)/3,y);}
      for(int x=6;x<=10;++x) pixel(x,13);
      for(int x=14;x<=20;++x) {pixel(x,9);pixel(x,12);pixel(x,16);}
      for(int y=7;y<=17;++y) pixel(17,y);
      g_variant_builder_add(&images,"(ii@ay)",size,size,
        g_variant_new_fixed_array(G_VARIANT_TYPE_BYTE,argb.data(),argb.size(),1));
    }
    return g_variant_builder_end(&images);
  }
  static GVariant *property(GDBusConnection *, const gchar *, const gchar *path,
                            const gchar *interface, const gchar *key, GError **,
                            gpointer data) {
    auto icon = static_cast<Icon *>(data);
    if (!strcmp(interface, "com.canonical.dbusmenu")) {
      if (!strcmp(key, "Version"))
        return g_variant_new_uint32(3);
      if (!strcmp(key, "TextDirection"))
        return g_variant_new_string("ltr");
      if (!strcmp(key, "Status"))
        return g_variant_new_string("normal");
      if (!strcmp(key, "IconThemePath"))
        return g_variant_new_strv(nullptr, 0);
    } else {
      if (!strcmp(key, "Category"))
        return g_variant_new_string("ApplicationStatus");
      if (!strcmp(key, "Id"))
        return g_variant_new_string("dome.translator");
      if (!strcmp(key, "Title"))
        return g_variant_new_string(icon->title.c_str());
      if (!strcmp(key, "Status"))
        return g_variant_new_string("Active");
      if (!strcmp(key, "IconName"))
        return g_variant_new_string(icon->icon.c_str());
      if (!strcmp(key, "IconThemePath"))
        return g_variant_new_string("");
      if (!strcmp(key, "WindowId"))
        return g_variant_new_uint32(0);
      if (!strcmp(key, "ItemIsMenu"))
        return g_variant_new_boolean(false);
      if (!strcmp(key, "Menu"))
        return g_variant_new_object_path((icon->path + "/Menu").c_str());
      if (!strcmp(key, "IconPixmap"))
        return pixmaps();
    }
    (void)path;
    return nullptr;
  }
  static void method(GDBusConnection *, const gchar *, const gchar *,
                     const gchar *interface, const gchar *method,
                     GVariant *args, GDBusMethodInvocation *reply,
                     gpointer data) {
    if (!strcmp(interface, name)) {
      const char *page;
      g_variant_get(args, "(&s)", &page);
      if (*page)
        static_cast<Impl *>(data)->show(page);
    } else {
      auto icon = static_cast<Icon *>(data);
      if (!strcmp(interface, "com.canonical.dbusmenu")) {
        if (!strcmp(method, "AboutToShow")) {
          g_dbus_method_invocation_return_value(reply,
                                                g_variant_new("(b)", FALSE));
          return;
        }
        if (!strcmp(method, "GetLayout")) {
          GVariantBuilder children;
          g_variant_builder_init(&children, G_VARIANT_TYPE("av"));
          const char *labels[] = {"Open translator", "Paste clipboard", "Paste primary selection", "Settings", "Quit", "Translate selected text"};
          for (int i = 0; i < 6; ++i) {
            GVariantBuilder p, c;
            g_variant_builder_init(&p, G_VARIANT_TYPE("a{sv}"));
            g_variant_builder_add(&p, "{sv}", "label",
                                  g_variant_new_string(labels[i]));
            g_variant_builder_add(&p, "{sv}", "enabled",
                                  g_variant_new_boolean(TRUE));
            if(i==5) {
              g_variant_builder_add(&p,"{sv}","toggle-type",g_variant_new_string("checkmark"));
              g_variant_builder_add(&p,"{sv}","toggle-state",g_variant_new_int32(icon->owner->selectionEnabled?1:0));
            }
            g_variant_builder_init(&c, G_VARIANT_TYPE("av"));
            g_variant_builder_add(&children, "v",
                                  g_variant_new("(i@a{sv}@av)", i + 1,
                                                g_variant_builder_end(&p),
                                                g_variant_builder_end(&c)));
          }
          GVariantBuilder props;
          g_variant_builder_init(&props, G_VARIANT_TYPE("a{sv}"));
          g_dbus_method_invocation_return_value(
              reply, g_variant_new("(u(i@a{sv}@av))", icon->owner->revision, 0,
                                   g_variant_builder_end(&props),
                                   g_variant_builder_end(&children)));
          return;
        }
        if (!strcmp(method, "Event")) {
          gint id;
          const char *event;
          GVariant *value;
          guint32 timestamp;
          g_variant_get(args, "(i&svu)", &id, &event, &value, &timestamp);
          g_variant_unref(value);
          const char *pages[] = {"open", "clipboard", "primary", "settings", "quit", "selection"};
          if (!strcmp(event, "clicked") && id >= 1 && id <= 6)
            icon->owner->show(pages[id - 1]);
        }
      } else if (strcmp(method, "Scroll"))
        icon->owner->show(icon->page);
    }
    g_dbus_method_invocation_return_value(reply, nullptr);
  }
  void add(const std::string &path, const char *xml, gpointer data) {
    auto info = g_dbus_node_info_new_for_xml(xml, nullptr);
    if (!info)
      throw std::runtime_error("Invalid tray interface");
    infos.push_back(info);
    static const GDBusInterfaceVTable table = {
        method, property, nullptr, {nullptr}};
    GError *error = nullptr;
    auto id = g_dbus_connection_register_object(
        bus, path.c_str(), info->interfaces[0], &table, data, nullptr, &error);
    if (!id) {
      std::string message = error->message;
      g_error_free(error);
      throw std::runtime_error(message);
    }
    registrations.push_back(id);
  }
  explicit Impl(std::function<void(const std::string &)> callback)
      : show(std::move(callback)) {
    bus = g_bus_get_sync(G_BUS_TYPE_SESSION, nullptr, nullptr);
    if (!bus)
      throw std::runtime_error("A session bus is required");
    auto reply = g_dbus_connection_call_sync(
        bus, "org.freedesktop.DBus", "/org/freedesktop/DBus",
        "org.freedesktop.DBus", "RequestName",
        g_variant_new("(su)", name, guint32(4)), G_VARIANT_TYPE("(u)"),
        G_DBUS_CALL_FLAGS_NONE, 2000, nullptr, nullptr);
    guint32 result = 0;
    if (reply) {
      g_variant_get(reply, "(u)", &result);
      g_variant_unref(reply);
    }
    if (result != 1 && result != 4)
      throw std::runtime_error("Another translator owns the session");
    add("/org/domed/Translator", control, this);
    icons = {{this, "/Translator", "open", "DomeD Translator", ""}};
    for (auto &icon : icons) {
      add(icon.path, item, &icon);
      add(icon.path + "/Menu", menu, &icon);
    }
    watcher = g_bus_watch_name_on_connection(
        bus, "org.kde.StatusNotifierWatcher", G_BUS_NAME_WATCHER_FLAGS_NONE,
        [](GDBusConnection *, const gchar *, const gchar *, gpointer p) {
          auto self = static_cast<Impl *>(p);
          for (auto &icon : self->icons)
            g_dbus_connection_call(
                self->bus, "org.kde.StatusNotifierWatcher",
                "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher",
                "RegisterStatusNotifierItem",
                g_variant_new("(s)", icon.path.c_str()), nullptr,
                G_DBUS_CALL_FLAGS_NONE, 2000, nullptr, nullptr, nullptr);
        },
        nullptr, this, nullptr);
  }
  ~Impl() {
    if (watcher)
      g_bus_unwatch_name(watcher);
    if (bus) {
      for (auto id : registrations)
        g_dbus_connection_unregister_object(bus, id);
      g_dbus_connection_call_sync(
          bus, "org.freedesktop.DBus", "/org/freedesktop/DBus",
          "org.freedesktop.DBus", "ReleaseName", g_variant_new("(s)", name),
          nullptr, G_DBUS_CALL_FLAGS_NONE, 1000, nullptr, nullptr);
      g_object_unref(bus);
    }
    for (auto info : infos)
      g_dbus_node_info_unref(info);
  }
};
Notifier::Notifier(std::function<void(const std::string &)> show)
    : impl(std::make_unique<Impl>(std::move(show))) {}
Notifier::~Notifier() = default;
void Notifier::setSelectionEnabled(bool enabled) {
  impl->selectionEnabled=enabled;++impl->revision;
  g_dbus_connection_emit_signal(impl->bus,nullptr,"/Translator/Menu","com.canonical.dbusmenu",
      "LayoutUpdated",g_variant_new("(ui)",impl->revision,0),nullptr);
}
bool Notifier::showExisting(const std::string &page) {
  auto bus = g_bus_get_sync(G_BUS_TYPE_SESSION, nullptr, nullptr);
  if (!bus)
    return false;
  auto reply = g_dbus_connection_call_sync(
      bus, name, "/org/domed/Translator", name, "Show",
      g_variant_new("(s)", page.c_str()), nullptr,
      G_DBUS_CALL_FLAGS_NO_AUTO_START, 1000, nullptr, nullptr);
  bool ok = reply;
  if (reply)
    g_variant_unref(reply);
  g_object_unref(bus);
  return ok;
}
