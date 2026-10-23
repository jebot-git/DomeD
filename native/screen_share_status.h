// SPDX-License-Identifier: MIT
#pragma once
#include <gio/gio.h>
#include <cstring>

// Keep the consent helper alive as a tray item. Its exit still revokes capture.
namespace screen_share {
inline const char xml[] = R"XML(<node>
<interface name="org.kde.StatusNotifierItem">
<property name="Category" type="s" access="read"/><property name="Id" type="s" access="read"/>
<property name="Title" type="s" access="read"/><property name="Status" type="s" access="read"/>
<property name="IconName" type="s" access="read"/><property name="IconThemePath" type="s" access="read"/>
<property name="WindowId" type="u" access="read"/><property name="ItemIsMenu" type="b" access="read"/>
<property name="Menu" type="o" access="read"/>
<method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
</interface><interface name="com.canonical.dbusmenu">
<property name="Version" type="u" access="read"/><property name="TextDirection" type="s" access="read"/>
<property name="Status" type="s" access="read"/><property name="IconThemePath" type="as" access="read"/>
<method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/>
<arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method>
<method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
<method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/></method>
</interface></node>)XML";
inline GVariant *properties(bool item) {
  GVariantBuilder b; g_variant_builder_init(&b, G_VARIANT_TYPE("a{sv}"));
  if (item) {
    g_variant_builder_add(&b,"{sv}","label",g_variant_new_string("Stop sharing"));
    g_variant_builder_add(&b,"{sv}","enabled",g_variant_new_boolean(true));
    g_variant_builder_add(&b,"{sv}","visible",g_variant_new_boolean(true));
    g_variant_builder_add(&b,"{sv}","icon-name",g_variant_new_string("media-playback-stop"));
  }
  return g_variant_builder_end(&b);
}
inline GVariant *empty_children() { return g_variant_new_array(G_VARIANT_TYPE_VARIANT,nullptr,0); }
inline GVariant *property(GDBusConnection *, const gchar *, const gchar *, const gchar *iface,
                          const gchar *key, GError **, gpointer) {
  bool menu = !strcmp(iface,"com.canonical.dbusmenu");
  if (!strcmp(key,"Status")) return g_variant_new_string(menu ? "normal" : "Active");
  if (!strcmp(key,"IconThemePath")) return menu ? g_variant_new_strv(nullptr,0) : g_variant_new_string("");
  if (!strcmp(key,"Version")) return g_variant_new_uint32(3);
  if (!strcmp(key,"TextDirection")) return g_variant_new_string("ltr");
  if (!strcmp(key,"Category")) return g_variant_new_string("SystemServices");
  if (!strcmp(key,"Id")) return g_variant_new_string("dome.screen-sharing");
  if (!strcmp(key,"Title")) return g_variant_new_string("Screen sharing active");
  if (!strcmp(key,"IconName")) return g_variant_new_string("media-record");
  if (!strcmp(key,"WindowId")) return g_variant_new_uint32(0);
  if (!strcmp(key,"ItemIsMenu")) return g_variant_new_boolean(true);
  if (!strcmp(key,"Menu")) return g_variant_new_object_path("/StatusNotifierItem/Menu");
  return nullptr;
}
inline void method(GDBusConnection *,const gchar *,const gchar *,const gchar *,const gchar *name,
                   GVariant *args,GDBusMethodInvocation *call,gpointer data) {
  if (!strcmp(name,"GetLayout")) {
    GVariantBuilder children;g_variant_builder_init(&children,G_VARIANT_TYPE("av"));
    g_variant_builder_add(&children,"v",g_variant_new("(i@a{sv}@av)",1,properties(true),empty_children()));
    g_dbus_method_invocation_return_value(call,g_variant_new("(u(i@a{sv}@av))",1,0,properties(false),g_variant_builder_end(&children)));
  } else if (!strcmp(name,"AboutToShow")) {
    g_dbus_method_invocation_return_value(call,g_variant_new("(b)",false));
  } else {
    if (!strcmp(name,"Event")) {
      gint32 id;const char *event;g_variant_get_child(args,0,"i",&id);g_variant_get_child(args,1,"&s",&event);
      if (id==1 && !strcmp(event,"clicked")) *static_cast<bool *>(data)=true;
    }
    g_dbus_method_invocation_return_value(call,nullptr);
  }
}
inline int run() {
  GError *error=nullptr;auto bus=g_bus_get_sync(G_BUS_TYPE_SESSION,nullptr,&error);
  if (!bus) {if(error)g_error_free(error);return 2;}
  auto info=g_dbus_node_info_new_for_xml(xml,nullptr);bool stopped=false;
  const GDBusInterfaceVTable table={method,property,nullptr,{nullptr}};
  auto item=g_dbus_connection_register_object(bus,"/StatusNotifierItem",info->interfaces[0],&table,&stopped,nullptr,nullptr);
  auto menu=g_dbus_connection_register_object(bus,"/StatusNotifierItem/Menu",info->interfaces[1],&table,&stopped,nullptr,nullptr);
  auto registered=g_dbus_connection_call_sync(bus,"org.kde.StatusNotifierWatcher","/StatusNotifierWatcher",
      "org.kde.StatusNotifierWatcher","RegisterStatusNotifierItem",
      g_variant_new("(s)",g_dbus_connection_get_unique_name(bus)),nullptr,G_DBUS_CALL_FLAGS_NONE,3000,nullptr,&error);
  if (registered) {
    g_variant_unref(registered);
    while (!stopped && !g_dbus_connection_is_closed(bus)) g_main_context_iteration(nullptr,true);
  } else if (error) {g_printerr("Screen sharing tray: %s\n",error->message);g_error_free(error);}
  if(menu)g_dbus_connection_unregister_object(bus,menu);
  if(item)g_dbus_connection_unregister_object(bus,item);
  g_dbus_node_info_unref(info);g_object_unref(bus);
  return registered ? 0 : 2;
}
}
