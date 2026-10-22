// SPDX-License-Identifier: MIT
#include "input-menu.h"
#include "services.h"
#include "keyboard-flags.h"
#include <FL/Fl.H>
#include <chrono>
#include <cstring>
#include <deque>
#include <future>
#include <map>
#include <stdexcept>
#include <vector>
namespace {
constexpr auto path = "/InputMethods/Menu";
constexpr auto interface = "com.canonical.dbusmenu";
constexpr auto xml = R"XML(<node><interface name="com.canonical.dbusmenu">
<property name="Version" type="u" access="read"/><property name="TextDirection" type="s" access="read"/><property name="Status" type="s" access="read"/><property name="IconThemePath" type="as" access="read"/>
<method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/><arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method>
<method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
<method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/></method>
<signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal>
</interface></node>)XML";
}
struct InputMenu::Impl {
  struct Node {
    int id, parent;
    std::string label, action, value, icon;
    bool enabled = true, radio = false, checked = false;
  };
  struct Reply { int parent, depth; GDBusMethodInvocation *invocation; };
  GDBusConnection *bus;
  guint registration;
  GDBusNodeInfo *info;
  std::function<void(const std::string &)> show;
  std::vector<Node> nodes;
  std::map<std::string, int> ids;
  std::vector<Reply> replies;
  std::deque<std::function<void()>> commands;
  std::future<services::Json> pending;
  std::string lastSnapshot;
  guint revision = 0;
  int nextId = 10;
  int id(const std::string &key) {
    auto it = ids.find(key);
    if (it != ids.end()) return it->second;
    return ids[key] = nextId++;
  }
  void refresh() {
    if (pending.valid()) return;
    auto work = std::move(commands);
    commands.clear();
    pending = std::async(std::launch::async, [work = std::move(work)] {
      for (auto &command : work) command();
      auto data = services::methods();
      data["activeGroup"] = services::inputGroup(data.at("group"));
      return data;
    });
  }
  void build(const services::Json &data, const std::string &error) {
    nodes = {{1,0,"Input methods","","",""},
             {2,0,"Input method groups","","",""},
             {3,0,"Configuration","","",""},
             {4,0,"Toggle input method","toggle","",""},
             {5,0,"Reload configuration","reload","",""},
             {6,3,"Fcitx5 configuration…","show","input",""},
             {7,3,"Global options…","show","input:fcitx://config/global",""}};
    if (!error.empty()) {
      nodes.push_back({8,1,error,"","","",false});
      nodes[3].enabled = nodes[4].enabled = false;
      return;
    }
    auto current = data.value("current", std::string());
    for (const auto &entry : data["activeGroup"]["methods"]) {
      auto method = entry.at("id").get<std::string>();
      auto label = method, language = std::string();
      for (const auto &available : data["methods"])
        if (available["id"] == method) {
          label = available.at("name"); language = available.value("language", "");
          break;
        }
      nodes.push_back({id("method:"+method),1,label,"select",method,
                       "flag-"+dx_flags::country(method,language),true,true,method==current});
    }
    for (const auto &group : data["groups"]) {
      auto name = group.get<std::string>();
      nodes.push_back({id("group:"+name),2,name,"group",name,"",true,true,group==data["group"]});
    }
    for (const auto &method : data["methods"])
      if (method["id"] == current && method.value("configurable",false))
        nodes.push_back({id("current-config"),3,"Current input method…","show",
                         "input:fcitx://config/inputmethod/"+current,""});
    for (const auto &addon : data["addons"]) {
      if (!addon.value("configurable",true)) continue;
      auto name = addon.at("id").get<std::string>();
      nodes.push_back({id("addon:"+name),3,addon.at("name").get<std::string>()+"…","show",
                       "input:fcitx://config/addon/"+name,""});
    }
  }
  GVariant *layout(int parent, int depth) {
    GVariantBuilder props, children;
    g_variant_builder_init(&props,G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_init(&children,G_VARIANT_TYPE("av"));
    for (const auto &node : nodes) if (node.id == parent) {
      g_variant_builder_add(&props,"{sv}","label",g_variant_new_string(node.label.c_str()));
      g_variant_builder_add(&props,"{sv}","enabled",g_variant_new_boolean(node.enabled));
      if (node.radio) {
        g_variant_builder_add(&props,"{sv}","toggle-type",g_variant_new_string("radio"));
        g_variant_builder_add(&props,"{sv}","toggle-state",g_variant_new_int32(node.checked ? 1 : 0));
      }
      if (node.action.empty() && node.enabled)
        g_variant_builder_add(&props,"{sv}","children-display",g_variant_new_string("submenu"));
      if (!node.icon.empty())
        g_variant_builder_add(&props,"{sv}","icon-name",g_variant_new_string(node.icon.c_str()));
    }
    if (depth != 0)
      for (const auto &node : nodes) if (node.parent == parent)
        g_variant_builder_add(&children,"v",layout(node.id,depth < 0 ? -1 : depth-1));
    return g_variant_new("(i@a{sv}@av)",parent,g_variant_builder_end(&props),g_variant_builder_end(&children));
  }
  static void tick(void *p) {
    auto self = static_cast<Impl *>(p);
    if (self->pending.valid() && self->pending.wait_for(std::chrono::seconds(0))==std::future_status::ready) {
      services::Json data;
      std::string error;
      try { data = self->pending.get(); }
      catch (const std::exception &e) { error = e.what(); }
      auto snapshot = data.dump() + error;
      if (snapshot != self->lastSnapshot) {
        self->build(data,error);
        self->lastSnapshot = std::move(snapshot);
        ++self->revision;
        g_dbus_connection_emit_signal(self->bus,nullptr,path,interface,"LayoutUpdated",
                                      g_variant_new("(ui)",self->revision,0),nullptr);
      }
      auto replies = std::move(self->replies); self->replies.clear();
      for (auto &reply : replies) {
        g_dbus_method_invocation_return_value(reply.invocation,
          g_variant_new("(u@(ia{sv}av))",self->revision,self->layout(reply.parent,reply.depth)));
        g_object_unref(reply.invocation);
      }
    }
    if (!self->commands.empty()) self->refresh();
    Fl::repeat_timeout(.04,tick,p);
  }
  static GVariant *property(GDBusConnection *,const gchar *,const gchar *,const gchar *,const gchar *key,GError **,gpointer) {
    if (!strcmp(key,"Version")) return g_variant_new_uint32(3);
    if (!strcmp(key,"TextDirection")) return g_variant_new_string("ltr");
    if (!strcmp(key,"Status")) return g_variant_new_string("normal");
    return g_variant_new_strv(nullptr,0);
  }
  static void method(GDBusConnection *,const gchar *,const gchar *,const gchar *,const gchar *name,
                     GVariant *args,GDBusMethodInvocation *reply,gpointer p) {
    auto self=static_cast<Impl *>(p);
    if (!strcmp(name,"GetLayout")) {
      int parent,depth; GVariant *properties;
      g_variant_get(args,"(ii@as)",&parent,&depth,&properties);g_variant_unref(properties);
      self->replies.push_back({parent,depth,static_cast<GDBusMethodInvocation *>(g_object_ref(reply))});
      self->refresh(); return;
    }
    if (!strcmp(name,"AboutToShow")) {
      g_dbus_method_invocation_return_value(reply,g_variant_new("(b)",FALSE)); return;
    }
    if (!strcmp(name,"Event")) {
      int id;const char *event;GVariant *value;guint32 time;
      g_variant_get(args,"(i&svu)",&id,&event,&value,&time);g_variant_unref(value);
      if (!strcmp(event,"clicked")) for (const auto &node : self->nodes)
        if (node.id==id && node.enabled && !node.action.empty()) {
          if (node.action=="show") self->show(node.value);
          else {
            auto action=node.action,v=node.value;
            self->commands.push_back([action,v]{services::inputAction(action,v);});
            self->refresh();
          }
          break;
        }
    }
    g_dbus_method_invocation_return_value(reply,nullptr);
  }
  Impl(GDBusConnection *connection,std::function<void(const std::string &)> callback)
      :bus(connection),show(std::move(callback)) {
    info=g_dbus_node_info_new_for_xml(xml,nullptr);
    static const GDBusInterfaceVTable table={method,property,nullptr,{nullptr}};
    registration=g_dbus_connection_register_object(bus,path,info->interfaces[0],&table,this,nullptr,nullptr);
    if (!registration) throw std::runtime_error("Cannot export Fcitx5 menu");
    build({},"Fcitx5 is starting…");
    Fl::add_timeout(.04,tick,this);
  }
  ~Impl() {
    Fl::remove_timeout(tick,this);
    for (auto &reply : replies) {
      g_dbus_method_invocation_return_dbus_error(reply.invocation,"org.domed.Tray.Closing","Manager is closing");
      g_object_unref(reply.invocation);
    }
    g_dbus_connection_unregister_object(bus,registration);g_dbus_node_info_unref(info);
  }
};
InputMenu::InputMenu(GDBusConnection *bus,std::function<void(const std::string &)> show)
    :impl(std::make_unique<Impl>(bus,std::move(show))) {}
InputMenu::~InputMenu()=default;
