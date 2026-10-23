// SPDX-License-Identifier: MIT
#include "services.h"
#include <algorithm>
#include <stdexcept>
namespace services {
static constexpr auto kde = "org.kde.kdeconnect";
static constexpr auto daemon = "/modules/kdeconnect";
static constexpr auto controller = "org.fcitx.Fcitx.Controller1";
static constexpr auto app = "DomeD Wallet Manager";
static GDBusConnection *connection() {
  static auto *bus = [] {
    GError *error = nullptr;
    auto *b = g_bus_get_sync(G_BUS_TYPE_SESSION, nullptr, &error);
    if (!b) {
      std::string message = error->message;
      g_error_free(error);
      throw std::runtime_error(message);
    }
    return b;
  }();
  return bus;
}
Variant call(const std::string &s, const std::string &p, const std::string &i,
             const std::string &m, GVariant *args) {
  if (!g_dbus_is_name(s.c_str()) || !g_variant_is_object_path(p.c_str()) ||
      !g_dbus_is_interface_name(i.c_str()) || !g_dbus_is_member_name(m.c_str()))
    throw std::runtime_error("Invalid service endpoint");
  GError *error = nullptr;
  auto *reply = g_dbus_connection_call_sync(
      connection(), s.c_str(), p.c_str(), i.c_str(), m.c_str(), args, nullptr,
      G_DBUS_CALL_FLAGS_NONE, 10000, nullptr, &error);
  if (!reply) {
    if (!error)
      throw std::runtime_error("Service call failed");
    std::string message = error->message;
    g_error_free(error);
    throw std::runtime_error(message);
  }
  return Variant(reply);
}
static Variant child(GVariant *v, size_t i) {
  return Variant(g_variant_get_child_value(v, i));
}
static std::string str(GVariant *v) { return g_variant_get_string(v, nullptr); }
static std::string first(GVariant *v) {
  auto c = child(v, 0);
  return str(c.get());
}
static Json strings(GVariant *v) {
  Json out = Json::array();
  for (size_t i = 0; i < g_variant_n_children(v); ++i) {
    auto c = child(v, i);
    out.push_back(str(c.get()));
  }
  return out;
}
static Variant fc(const std::string &method, GVariant *args = nullptr) {
  return call("org.fcitx.Fcitx5", "/controller", controller, method, args);
}
static Variant kw(const std::string &method, GVariant *args = nullptr) {
  // Current kwalletd exports all aliases; older installations use kwalletd5.
  static std::string service;
  if (service.empty()) {
    for (auto name :
         {"org.kde.kwalletd6", "org.kde.kwalletd5", "org.kde.kwalletd"}) {
      auto r = call("org.freedesktop.DBus", "/org/freedesktop/DBus",
                    "org.freedesktop.DBus", "ListActivatableNames");
      auto names = child(r.get(), 0);
      auto list = strings(names.get());
      auto owned = call("org.freedesktop.DBus", "/org/freedesktop/DBus",
                        "org.freedesktop.DBus", "NameHasOwner",
                        g_variant_new("(s)", name));
      gboolean yes;
      g_variant_get(owned.get(), "(b)", &yes);
      if (yes || std::find(list.begin(), list.end(), name) != list.end()) {
        service = name;
        break;
      }
    }
    if (service.empty())
      throw std::runtime_error("KWallet service is not installed");
  }
  auto path = service == "org.kde.kwalletd6"   ? "/modules/kwalletd6"
              : service == "org.kde.kwalletd5" ? "/modules/kwalletd5"
                                               : "/modules/kwalletd";
  return call(service, path, "org.kde.KWallet", method, args);
}
static Json properties(const std::string &s, const std::string &path,
                       const std::string &iface) {
  auto r = call(s, path, "org.freedesktop.DBus.Properties", "GetAll",
                g_variant_new("(s)", iface.c_str()));
  auto dict = child(r.get(), 0);
  GVariantIter iter;
  g_variant_iter_init(&iter, dict.get());
  const char *key;
  GVariant *value;
  Json out = Json::object();
  while (g_variant_iter_next(&iter, "{&sv}", &key, &value)) {
    Variant guard(value);
    if (g_variant_is_of_type(value, G_VARIANT_TYPE_STRING) ||
        g_variant_is_of_type(value, G_VARIANT_TYPE_OBJECT_PATH))
      out[key] = str(value);
    else if (g_variant_is_of_type(value, G_VARIANT_TYPE_BOOLEAN))
      out[key] = bool(g_variant_get_boolean(value));
    else if (g_variant_is_of_type(value, G_VARIANT_TYPE_INT32))
      out[key] = g_variant_get_int32(value);
    else if (g_variant_is_of_type(value, G_VARIANT_TYPE("as")))
      out[key] = strings(value);
  }
  return out;
}
Json devices() {
  auto r = call(kde, daemon, "org.kde.kdeconnect.daemon", "devices",
                g_variant_new("(bb)", FALSE, FALSE));
  auto names = child(r.get(), 0);
  Json out = Json::array();
  for (auto &id : strings(names.get())) {
    auto p = properties(
        kde, std::string(daemon) + "/devices/" + id.get<std::string>(),
        "org.kde.kdeconnect.device");
    p["id"] = id;
    out.push_back(p);
  }
  return out;
}
Json device(const std::string &id) {
  auto path = std::string(daemon) + "/devices/" + id;
  auto p = properties(kde, path, "org.kde.kdeconnect.device");
  try {
    p["battery"] =
        properties(kde, path + "/battery", "org.kde.kdeconnect.device.battery");
  } catch (...) {
  }
  auto r = call(kde, path, "org.kde.kdeconnect.device", "loadedPlugins");
  auto names = child(r.get(), 0);
  p["plugins"] = strings(names.get());
  return p;
}
void deviceAction(const std::string &id, const std::string &action,
                  const std::string &value) {
  if (action == "refresh") {
    call(kde, daemon, "org.kde.kdeconnect.daemon", "forceOnNetworkChange");
    return;
  }
  if (action == "rename") {
    call(kde, daemon, "org.kde.kdeconnect.daemon", "setAnnouncedName",
         g_variant_new("(s)", value.c_str()));
    return;
  }
  auto path = std::string(daemon) + "/devices/" + id;
  if (action == "pair" || action == "accept" || action == "cancel" ||
      action == "unpair") {
    auto m = action == "pair"     ? "requestPairing"
             : action == "accept" ? "acceptPairing"
             : action == "cancel" ? "cancelPairing"
                                  : "unpair";
    call(kde, path, "org.kde.kdeconnect.device", m);
    return;
  }
  if (action == "plugin-on" || action == "plugin-off") {
    call(kde, path, "org.kde.kdeconnect.device", "setPluginEnabled",
         g_variant_new("(sb)", value.c_str(), action == "plugin-on"));
    return;
  }
  const auto plugin = action == "ring"   ? "findmyphone"
                      : action == "ping" ? "ping"
                                         : "share";
  const auto method = action == "ring"   ? "ring"
                      : action == "ping" ? "sendPing"
                      : action == "text" ? "shareText"
                                         : "shareUrl";
  call(kde, path + "/" + plugin,
       std::string("org.kde.kdeconnect.device.") + plugin, method,
       action == "ring" ? nullptr : g_variant_new("(s)", value.c_str()));
}
Json methods() {
  auto r = fc("AvailableInputMethods");
  auto list = child(r.get(), 0);
  Json out;
  out["methods"] = Json::array();
  for (size_t i = 0; i < g_variant_n_children(list.get()); ++i) {
    auto item = child(list.get(), i);
    auto id = child(item.get(), 0), name = child(item.get(), 1),
         lang = child(item.get(), 5);
    auto configurable = child(item.get(), 6);
    out["methods"].push_back({{"id", str(id.get())},
                              {"name", str(name.get())},
                              {"language", str(lang.get())},
                              {"configurable", bool(g_variant_get_boolean(configurable.get()))}});
  }
  r = fc("CurrentInputMethod");
  out["current"] = first(r.get());
  r = fc("CurrentInputMethodGroup");
  out["group"] = first(r.get());
  r = fc("InputMethodGroups");
  list = child(r.get(), 0);
  out["groups"] = strings(list.get());
  out["addons"] = Json::array();
  try {
    r = fc("GetAddons");
    list = child(r.get(), 0);
    for (size_t i = 0; i < g_variant_n_children(list.get()); ++i) {
      auto a = child(list.get(), i);
      auto id = child(a.get(), 0), name = child(a.get(), 1);
      auto configurable = child(a.get(), 4);
      out["addons"].push_back(
          {{"id", str(id.get())}, {"name", str(name.get())},
           {"configurable", bool(g_variant_get_boolean(configurable.get()))}});
    }
  } catch (...) {
  }
  return out;
}
void inputAction(const std::string &action, const std::string &value) {
  if (action == "select")
    fc("SetCurrentIM", g_variant_new("(s)", value.c_str()));
  else if (action == "group")
    fc("SwitchInputMethodGroup", g_variant_new("(s)", value.c_str()));
  else if (action == "add-group")
    fc("AddInputMethodGroup", g_variant_new("(s)", value.c_str()));
  else if (action == "remove-group")
    fc("RemoveInputMethodGroup", g_variant_new("(s)", value.c_str()));
  else if (action == "toggle")
    fc("Toggle");
  else if (action == "activate")
    fc("Activate");
  else if (action == "deactivate")
    fc("Deactivate");
  else if (action == "reload")
    fc("ReloadConfig");
  else if (action == "save")
    fc("Save");
  else
    throw std::runtime_error("Unknown input method action");
}
Json inputGroup(const std::string &group) {
  auto r = fc("InputMethodGroupInfo", g_variant_new("(s)", group.c_str()));
  auto layout = child(r.get(), 0), list = child(r.get(), 1);
  Json out = {{"layout", str(layout.get())}, {"methods", Json::array()}};
  for (size_t i = 0; i < g_variant_n_children(list.get()); ++i) {
    auto v = child(list.get(), i);
    auto id = child(v.get(), 0), l = child(v.get(), 1);
    out["methods"].push_back({{"id", str(id.get())}, {"layout", str(l.get())}});
  }
  return out;
}
void saveGroup(const std::string &group, const std::string &layout,
               const Json &methods) {
  GVariantBuilder list;
  g_variant_builder_init(&list, G_VARIANT_TYPE("a(ss)"));
  for (auto &m : methods)
    g_variant_builder_add(&list, "(ss)", m.at("id").get<std::string>().c_str(),
                          m.at("layout").get<std::string>().c_str());
  fc("SetInputMethodGroupInfo",
     g_variant_new("(ss@a(ss))", group.c_str(), layout.c_str(),
                   g_variant_builder_end(&list)));
  fc("Save");
}
// Preserve Fcitx's nested RawConfig dictionary and edit only leaf strings.
static void flatten(GVariant *value, const std::string &path, Json &out) {
  if (g_variant_is_of_type(value, G_VARIANT_TYPE_VARIANT)) {
    Variant v(g_variant_get_variant(value));
    flatten(v.get(), path, out);
  } else if (g_variant_is_of_type(value, G_VARIANT_TYPE_STRING))
    out[path] = str(value);
  else if (g_variant_is_of_type(value, G_VARIANT_TYPE("a{sv}"))) {
    GVariantIter iter;
    g_variant_iter_init(&iter, value);
    const char *key;
    GVariant *v;
    while (g_variant_iter_next(&iter, "{&sv}", &key, &v)) {
      Variant guard(v);
      flatten(v, path.empty() ? key : path + "/" + key, out);
    }
  }
}
static GVariant *rebuild(GVariant *value, const std::string &path,
                         const Json &values) {
  if (g_variant_is_of_type(value, G_VARIANT_TYPE_VARIANT)) {
    Variant v(g_variant_get_variant(value));
    return g_variant_new_variant(rebuild(v.get(), path, values));
  }
  if (g_variant_is_of_type(value, G_VARIANT_TYPE_STRING))
    return g_variant_new_string(values.value(path, str(value)).c_str());
  if (g_variant_is_of_type(value, G_VARIANT_TYPE("a{sv}"))) {
    GVariantBuilder builder;
    g_variant_builder_init(&builder, G_VARIANT_TYPE("a{sv}"));
    GVariantIter iter;
    g_variant_iter_init(&iter, value);
    const char *key;
    GVariant *v;
    while (g_variant_iter_next(&iter, "{&sv}", &key, &v)) {
      Variant guard(v);
      g_variant_builder_add(
          &builder, "{sv}", key,
          rebuild(v, path.empty() ? key : path + "/" + key, values));
    }
    return g_variant_builder_end(&builder);
  }
  return g_variant_ref(value);
}
Json inputConfig(const std::string &uri) {
  auto r = fc("GetConfig", g_variant_new("(s)", uri.c_str()));
  auto data = child(r.get(), 0);
  Json out = Json::object();
  flatten(data.get(), "", out);
  return out;
}
void saveInputConfig(const std::string &uri, const Json &values) {
  auto r = fc("GetConfig", g_variant_new("(s)", uri.c_str()));
  auto data = child(r.get(), 0);
  Variant raw(g_variant_get_variant(data.get()));
  fc("SetConfig",
     g_variant_new("(sv)", uri.c_str(), rebuild(raw.get(), "", values)));
  fc("Save");
}
Json wallets() {
  auto r = kw("wallets");
  auto list = child(r.get(), 0);
  return strings(list.get());
}
void walletLifecycle(const std::string &action, const std::string &name) {
  if (action == "password") {
    kw("changePassword", g_variant_new("(sxs)", name.c_str(), gint64(0), app));
    return;
  }
  auto r = kw("deleteWallet", g_variant_new("(s)", name.c_str()));
  gint status;
  g_variant_get(r.get(), "(i)", &status);
  if (status)
    throw std::runtime_error("Wallet deletion failed");
}
int openWallet(const std::string &name) {
  auto r = kw("open", g_variant_new("(sxs)", name.c_str(), gint64(0), app));
  gint handle;
  g_variant_get(r.get(), "(i)", &handle);
  if (handle < 0)
    throw std::runtime_error("Wallet unlock was cancelled or denied");
  return handle;
}
Json folders(int handle) {
  auto r = kw("folderList", g_variant_new("(is)", handle, app));
  auto list = child(r.get(), 0);
  return strings(list.get());
}
Json entries(int handle, const std::string &folder) {
  auto r = kw("entryList", g_variant_new("(iss)", handle, folder.c_str(), app));
  auto list = child(r.get(), 0);
  Json out = Json::array();
  for (auto &key : strings(list.get())) {
    auto t =
        kw("entryType", g_variant_new("(isss)", handle, folder.c_str(),
                                      key.get<std::string>().c_str(), app));
    gint type;
    g_variant_get(t.get(), "(i)", &type);
    out.push_back({{"key", key}, {"type", type}});
  }
  return out;
}
static void putWord(std::vector<unsigned char> &bytes, uint32_t value) {
  for (int shift : {24, 16, 8, 0})
    bytes.push_back((value >> shift) & 255);
}
static uint32_t getWord(const std::vector<unsigned char> &bytes,
                        size_t &offset) {
  if (offset + 4 > bytes.size())
    throw std::runtime_error("Truncated wallet map");
  uint32_t value = 0;
  for (int i = 0; i < 4; ++i)
    value = (value << 8) | bytes[offset++];
  return value;
}
static void qstring(std::vector<unsigned char> &bytes,
                    const std::string &text) {
  glong count = 0;
  auto utf16 =
      g_utf8_to_utf16(text.c_str(), text.size(), nullptr, &count, nullptr);
  if (!utf16)
    throw std::runtime_error("Invalid UTF-8 in wallet map");
  putWord(bytes, count * 2);
  for (glong i = 0; i < count; ++i) {
    bytes.push_back(utf16[i] >> 8);
    bytes.push_back(utf16[i] & 255);
  }
  g_free(utf16);
}
static std::string qstring(const std::vector<unsigned char> &bytes,
                           size_t &offset) {
  auto size = getWord(bytes, offset);
  if (size == 0xffffffff || size == 0)
    return {};
  if (size % 2 || size > bytes.size() - offset)
    throw std::runtime_error("Invalid wallet map string");
  std::vector<gunichar2> utf16;
  for (size_t i = 0; i < size; i += 2)
    utf16.push_back((bytes[offset + i] << 8) | bytes[offset + i + 1]);
  offset += size;
  auto text =
      g_utf16_to_utf8(utf16.data(), utf16.size(), nullptr, nullptr, nullptr);
  if (!text)
    throw std::runtime_error("Invalid UTF-16 in wallet map");
  std::string out = text;
  g_free(text);
  return out;
}
Json readData(int handle, const std::string &folder, const std::string &key) {
  auto t = kw("entryType", g_variant_new("(isss)", handle, folder.c_str(),
                                         key.c_str(), app));
  gint type;
  g_variant_get(t.get(), "(i)", &type);
  if (type == 1)
    return {{"type", 1}, {"data", readPassword(handle, folder, key)}};
  if (type != 2 && type != 3)
    throw std::runtime_error("Unknown wallet entry type");
  auto r =
      kw(type == 3 ? "readMap" : "readEntry",
         g_variant_new("(isss)", handle, folder.c_str(), key.c_str(), app));
  auto v = child(r.get(), 0);
  gsize size;
  auto bytes = static_cast<const unsigned char *>(
      g_variant_get_fixed_array(v.get(), &size, 1));
  if (size > 16 * 1024 * 1024)
    throw std::runtime_error("Wallet data exceeds 16 MiB");
  if (type == 2) {
    static const char hex[] = "0123456789abcdef";
    std::string data;
    for (size_t i = 0; i < size; ++i) {
      data += hex[bytes[i] >> 4];
      data += hex[bytes[i] & 15];
    }
    return {{"type", 2}, {"data", data}};
  }
  std::vector<unsigned char> data(bytes, bytes + size);
  size_t offset = 0;
  auto count = getWord(data, offset);
  if (count > data.size() / 8)
    throw std::runtime_error("Invalid wallet map length");
  Json map = Json::object();
  for (uint32_t i = 0; i < count; ++i) {
    auto k = qstring(data, offset);
    map[k] = qstring(data, offset);
  }
  if (offset != data.size())
    throw std::runtime_error("Unexpected wallet map data");
  return {{"type", 3}, {"data", map.dump(2)}};
}
void writeData(int handle, const std::string &folder, const std::string &key,
               const Json &data) {
  int type = data.at("type");
  std::string text = data.at("data");
  if (type == 1) {
    walletAction("write", handle, folder, key, text);
    return;
  }
  std::vector<unsigned char> bytes;
  if (type == 3) {
    auto map = Json::parse(text);
    if (!map.is_object())
      throw std::runtime_error("Map entries must be a JSON object of strings");
    putWord(bytes, map.size());
    for (auto it = map.begin(); it != map.end(); ++it) {
      qstring(bytes, it.key());
      qstring(bytes, it.value().get<std::string>());
    }
  } else if (type == 2) {
    std::string hex;
    for (unsigned char c : text)
      if (!isspace(c))
        hex += c;
    if (hex.size() % 2)
      throw std::runtime_error("Enter an even number of hexadecimal digits");
    auto digit = [](char c) {
      if (c >= '0' && c <= '9')
        return c - '0';
      if (c >= 'a' && c <= 'f')
        return c - 'a' + 10;
      if (c >= 'A' && c <= 'F')
        return c - 'A' + 10;
      throw std::runtime_error("Invalid hexadecimal digit");
    };
    for (size_t i = 0; i < hex.size(); i += 2)
      bytes.push_back((digit(hex[i]) << 4) | digit(hex[i + 1]));
  } else
    throw std::runtime_error("Unknown wallet entry type");
  if (bytes.size() > 16 * 1024 * 1024)
    throw std::runtime_error("Wallet data exceeds 16 MiB");
  auto array = g_variant_new_fixed_array(G_VARIANT_TYPE_BYTE, bytes.data(),
                                         bytes.size(), 1);
  auto args = type == 3 ? g_variant_new("(iss@ays)", handle, folder.c_str(),
                                        key.c_str(), array, app)
                        : g_variant_new("(iss@ayis)", handle, folder.c_str(),
                                        key.c_str(), array, type, app);
  auto r = kw(type == 3 ? "writeMap" : "writeEntry", args);
  gint status;
  g_variant_get(r.get(), "(i)", &status);
  if (status)
    throw std::runtime_error("Wallet data write failed");
}
std::string readPassword(int handle, const std::string &folder,
                         const std::string &key) {
  auto r = kw("readPassword", g_variant_new("(isss)", handle, folder.c_str(),
                                            key.c_str(), app));
  return first(r.get());
}
void walletAction(const std::string &action, int handle,
                  const std::string &folder, const std::string &key,
                  const std::string &value) {
  if (action == "lock") {
    auto r = kw("close", g_variant_new("(ibs)", handle, FALSE, app));
    gint status;
    g_variant_get(r.get(), "(i)", &status);
    if (status)
      throw std::runtime_error("Wallet is still used by another application");
    return;
  }
  if (action == "create-folder" || action == "remove-folder") {
    auto r = kw(action == "create-folder" ? "createFolder" : "removeFolder",
                g_variant_new("(iss)", handle, folder.c_str(), app));
    gboolean ok;
    g_variant_get(r.get(), "(b)", &ok);
    if (!ok)
      throw std::runtime_error("Folder operation failed");
    return;
  }
  auto args =
      action == "write" ? g_variant_new("(issss)", handle, folder.c_str(),
                                        key.c_str(), value.c_str(), app)
      : action == "rename"
          ? g_variant_new("(issss)", handle, folder.c_str(), key.c_str(),
                          value.c_str(), app)
          : g_variant_new("(isss)", handle, folder.c_str(), key.c_str(), app);
  auto r = kw(action == "write"    ? "writePassword"
              : action == "rename" ? "renameEntry"
                                   : "removeEntry",
              args);
  gint status;
  g_variant_get(r.get(), "(i)", &status);
  if (status)
    throw std::runtime_error("Wallet entry operation failed");
}
std::string frontend(const std::string &identity) {
  auto name = identity;
  std::transform(name.begin(), name.end(), name.begin(), ::tolower);
  if (name.find("kdeconnect") != std::string::npos ||
      name.find("kde connect") != std::string::npos)
    return "devices";
  if (name.find("fcitx") != std::string::npos)
    return "input";
  if (name.find("kwallet") != std::string::npos ||
      name.find("wallet manager") != std::string::npos)
    return "wallets";
  return {};
}
static bool owned(const char *name) {
  auto reply = call("org.freedesktop.DBus", "/org/freedesktop/DBus",
                    "org.freedesktop.DBus", "NameHasOwner", g_variant_new("(s)", name));
  gboolean yes = FALSE;
  g_variant_get(reply.get(), "(b)", &yes);
  return yes;
}
static Json suiteItem(const char *id, const char *title) {
  return {{"Id", id}, {"Title", title}, {"suite", id}};
}
static std::string placementPath() {
  return std::string(g_get_user_config_dir()) + "/dome/tray-applications.json";
}
static Json placements() {
  gchar *contents = nullptr; gsize length = 0;
  if (!g_file_get_contents(placementPath().c_str(), &contents, &length, nullptr)) return Json::object();
  std::string text(contents, length); g_free(contents);
  if (length > 65536) throw std::runtime_error("Tray placement configuration is too large");
  auto data = Json::parse(text);
  if (!data.is_object()) throw std::runtime_error("Invalid tray placement configuration");
  for (auto &value : data.items())
    if (!value.value().is_boolean()) throw std::runtime_error("Invalid tray placement value");
  return data;
}
static bool movable(const Json &item) {
  auto id = item.value("Id", std::string());
  auto identity = id + " " + item.value("Title", std::string());
  std::transform(identity.begin(), identity.end(), identity.begin(), ::tolower);
  return !id.empty() && id != "dome.tray" && identity.find("fcitx") == std::string::npos;
}
void trayPlacement(const Json &item, bool managed) {
  if (!movable(item)) throw std::runtime_error("This session tray item has a fixed placement");
  auto id = item.at("Id").get<std::string>();
  if (id.size() > 256) throw std::runtime_error("Invalid tray application identity");
  auto data = placements(); data[id] = managed;
  auto path = placementPath();
  auto parent = g_path_get_dirname(path.c_str());
  if (g_mkdir_with_parents(parent, 0700) != 0) { g_free(parent); throw std::runtime_error("Cannot create tray settings directory"); }
  g_free(parent);
  auto file = g_file_new_for_path(path.c_str());
  auto contents = data.dump(2) + "\n";
  if (contents.size() > 65536) { g_object_unref(file); throw std::runtime_error("Too many tray placement settings"); }
  GError *error = nullptr;
  auto ok = g_file_replace_contents(file, contents.data(), contents.size(), nullptr, FALSE,
      G_FILE_CREATE_PRIVATE, nullptr, nullptr, &error);
  g_object_unref(file);
  if (!ok) { std::string message = error->message; g_error_free(error); throw std::runtime_error(message); }
}
Json trayItems() {
  Json out = Json::array();
  auto *translator = g_find_program_in_path("dome-translator");
  if (translator || owned("org.domed.Translator"))
    out.push_back(suiteItem("dome.translator", "DomeD Translator"));
  g_free(translator);
  out.push_back(suiteItem("dome.screenshot", "Screenshot (PrintScreen)"));
  const auto settings = placements();
  auto annotate = [&](Json &item) {
    auto id = item.value("Id", std::string());
    item["movable"] = movable(item);
    item["managed"] = settings.value(id, id == "dome.translator" || id == "wivrn.pro");
  };
  for (auto &item : out) annotate(item);
  if (!owned("org.kde.StatusNotifierWatcher")) return out;
  auto r = call("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher",
                "org.freedesktop.DBus.Properties", "Get",
                g_variant_new("(ss)", "org.kde.StatusNotifierWatcher",
                              "RegisteredStatusNotifierItems"));
  auto wrapped = child(r.get(), 0);
  Variant list(g_variant_get_variant(wrapped.get()));
  for (auto &key : strings(list.get())) {
    auto k = key.get<std::string>();
    auto slash = k.find('/');
    if (slash == std::string::npos)
      continue;
    auto service = k.substr(0, slash), path = k.substr(slash);
    if (!g_dbus_is_name(service.c_str()) ||
        !g_variant_is_object_path(path.c_str()))
      continue;
    try {
      auto item = properties(service, path, "org.kde.StatusNotifierItem");
      if (item.value("Id", "") == "dome.translator") continue;
      item["service"] = service;
      item["path"] = path;
      annotate(item);
      if (item.value("Status", "") != "Passive")
        out.push_back(item);
    } catch (...) {
    }
  }
  return out;
}
Json trayMenu(const Json &item, int parent) {
  auto native = frontend(item.value("Id", std::string()) + " " + item.value("Title", std::string()));
  if (!native.empty())
    return Json::array({{{"id", -1}, {"label", "Open native controls"}, {"enabled", true}}});
  const auto suite = item.value("suite", std::string());
  if (!suite.empty()) {
    Json rows = Json::array();
    const std::vector<std::string> labels = suite == "dome.translator"
        ? std::vector<std::string>{"Open translator", "Paste clipboard", "Paste primary selection",
                                   "Settings", "Quit", "Translate selected text"}
        : std::vector<std::string>{"Take screenshot"};
    for (size_t i = 0; i < labels.size(); ++i)
      rows.push_back({{"id", i + 1}, {"label", labels[i]}, {"enabled", suite == "dome.translator" ? i != 4 : owned("org.domed.Compositor")}});
    if (suite == "dome.translator" && owned("org.domed.Translator")) {
      // Use the live menu to preserve its selection toggle and Quit action.
      return trayMenu({{"service", "org.domed.Translator"}, {"Menu", "/Translator/Menu"}}, parent);
    }
    return rows;
  }
  auto service = item.at("service").get<std::string>(),
       path = item.value("Menu", std::string());
  if (path.empty() || path == "/")
    throw std::runtime_error("This application does not export a tray menu");
  try {
    call(service, path, "com.canonical.dbusmenu", "AboutToShow",
         g_variant_new("(i)", parent));
  } catch (...) {
  }
  auto r =
      call(service, path, "com.canonical.dbusmenu", "GetLayout",
           g_variant_new("(ii@as)", parent, 1, g_variant_new_strv(nullptr, 0)));
  auto root = child(r.get(), 1), children = child(root.get(), 2);
  Json out = Json::array();
  for (size_t i = 0; i < g_variant_n_children(children.get()); ++i) {
    auto v = child(children.get(), i);
    Variant node(g_variant_get_variant(v.get()));
    auto id = child(node.get(), 0), props = child(node.get(), 1);
    const char *label = "", *type = "", *toggle = "", *display = "";
    gboolean enabled = TRUE, visible = TRUE;
    gint checked = 0;
    g_variant_lookup(props.get(), "label", "&s", &label);
    g_variant_lookup(props.get(), "type", "&s", &type);
    g_variant_lookup(props.get(), "toggle-type", "&s", &toggle);
    g_variant_lookup(props.get(), "children-display", "&s", &display);
    g_variant_lookup(props.get(), "enabled", "b", &enabled);
    g_variant_lookup(props.get(), "visible", "b", &visible);
    g_variant_lookup(props.get(), "toggle-state", "i", &checked);
    if (visible)
      out.push_back({{"id", g_variant_get_int32(id.get())},
                     {"label", label},
                     {"separator", std::string(type) == "separator"},
                     {"enabled", bool(enabled)},
                     {"checkable", bool(*toggle)},
                     {"checked", checked == 1},
                     {"submenu", std::string(display) == "submenu"}});
  }
  return out;
}
void trayEvent(const Json &item, int id) {
  const auto suite = item.value("suite", std::string());
  if (suite == "dome.screenshot") {
    if (id != 1) throw std::runtime_error("Unknown screenshot action");
    call("org.domed.Compositor", "/org/domed/Compositor", "org.domed.Compositor", "Screenshot", nullptr);
    return;
  }
  if (suite == "dome.translator") {
    if (owned("org.domed.Translator")) {
      trayEvent({{"service", "org.domed.Translator"}, {"Menu", "/Translator/Menu"}}, id);
      return;
    }
    const char *options[] = {nullptr, "--clipboard", "--primary", "--settings", nullptr, "--selection"};
    if (id == 5) return;
    if (id < 1 || id > 6) throw std::runtime_error("Unknown translator action");
    const gchar *args[] = {"dome-translator", options[id - 1], nullptr};
    GError *error = nullptr;
    auto *process = g_subprocess_newv(args, G_SUBPROCESS_FLAGS_NONE, &error);
    if (!process) {
      std::string message = error->message;
      g_error_free(error);
      throw std::runtime_error(message);
    }
    g_object_unref(process);
    return;
  }
  call(item.at("service"), item.at("Menu"), "com.canonical.dbusmenu", "Event",
       g_variant_new("(isvu)", id, "clicked", g_variant_new_int32(0),
                     guint32(0)));
}
} // namespace services
