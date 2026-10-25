// SPDX-License-Identifier: MIT
#include "bluetooth.h"
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <functional>
#include <gio/gio.h>
#include <map>
#include <mutex>
#include <set>
#include <thread>
namespace bluetooth {
namespace {
constexpr auto agentPath = "/org/domed/Tray/BluetoothAgent";
constexpr auto agentXml = R"(<node><interface name="org.bluez.Agent1">
<method name="Release"/><method name="Cancel"/>
<method name="RequestPinCode"><arg type="o" direction="in"/><arg type="s" direction="out"/></method>
<method name="DisplayPinCode"><arg type="o" direction="in"/><arg type="s" direction="in"/></method>
<method name="RequestPasskey"><arg type="o" direction="in"/><arg type="u" direction="out"/></method>
<method name="DisplayPasskey"><arg type="o" direction="in"/><arg type="u" direction="in"/><arg type="q" direction="in"/></method>
<method name="RequestConfirmation"><arg type="o" direction="in"/><arg type="u" direction="in"/></method>
<method name="RequestAuthorization"><arg type="o" direction="in"/></method>
<method name="AuthorizeService"><arg type="o" direction="in"/><arg type="s" direction="in"/></method>
</interface></node>)";
Json properties(GVariant *dict) {
  Json out = Json::object();
  GVariantIter it;
  g_variant_iter_init(&it, dict);
  const char *key;
  GVariant *value;
  while (g_variant_iter_next(&it, "{&sv}", &key, &value)) {
    if (g_variant_is_of_type(value, G_VARIANT_TYPE_STRING) ||
        g_variant_is_of_type(value, G_VARIANT_TYPE_OBJECT_PATH))
      out[key] = g_variant_get_string(value, nullptr);
    else if (g_variant_is_of_type(value, G_VARIANT_TYPE_BOOLEAN))
      out[key] = bool(g_variant_get_boolean(value));
    else if (g_variant_is_of_type(value, G_VARIANT_TYPE_BYTE))
      out[key] = g_variant_get_byte(value);
    else if (g_variant_is_of_type(value, G_VARIANT_TYPE_INT16))
      out[key] = g_variant_get_int16(value);
    g_variant_unref(value);
  }
  return out;
}
} // namespace
struct Client::Impl {
  GMainContext *context = g_main_context_new();
  GMainLoop *loop = g_main_loop_new(context, FALSE);
  GDBusConnection *bus = nullptr;
  GCancellable *cancel = g_cancellable_new();
  GDBusNodeInfo *info = nullptr;
  guint watch = 0, object = 0, signals = 0;
  GSource *timer = nullptr;
  std::thread worker;
  mutable std::mutex mutex;
  Json state = {{"available", false},
                {"adapters", Json::array()},
                {"devices", Json::array()},
                {"prompt", nullptr},
                {"error", "Connecting to Bluetooth service…"},
                {"agent", false},
                {"busy", Json::array()},
                {"pairing", Json::array()}};
  std::string owner;
  std::set<std::string> scans, busy, pairing;
  bool allowScanning = true;
  std::map<std::string, gint64> deadlines;
  GDBusMethodInvocation *request = nullptr;
  std::string requestKind;
  unsigned serial = 0;
  gint64 promptDeadline = 0;
  bool closing = false, refreshing = false, dirty = false;
  int inflight = 0;
  using Done = std::function<void(GVariant *, const std::string &)>;
  struct Call {
    Impl *self;
    Done done;
  };
  void error(const std::string &text) {
    std::lock_guard lock(mutex);
    state["error"] = text;
  }
  void post(std::function<void()> fn) {
    auto job = new std::function<void()>(std::move(fn));
    auto source = g_idle_source_new();
    g_source_set_callback(
        source,
        [](gpointer p) -> gboolean {
          (*static_cast<std::function<void()> *>(p))();
          return G_SOURCE_REMOVE;
        },
        job,
        [](gpointer p) { delete static_cast<std::function<void()> *>(p); });
    g_source_attach(source, context);
    g_source_unref(source);
  }
  void call(const std::string &path, const char *iface, const char *method,
            GVariant *args = nullptr, Done done = {}, int timeout = 10000) {
    ++inflight;
    g_dbus_connection_call(
        bus, "org.bluez", path.c_str(), iface, method, args, nullptr,
        G_DBUS_CALL_FLAGS_NONE, timeout, cancel,
        [](GObject *source, GAsyncResult *result, gpointer data) {
          std::unique_ptr<Call> c(static_cast<Call *>(data));
          GError *err = nullptr;
          auto value = g_dbus_connection_call_finish(G_DBUS_CONNECTION(source),
                                                     result, &err);
          std::string text = err ? err->message : "";
          if (err)
            g_error_free(err);
          --c->self->inflight;
          if (!c->self->closing && c->done)
            c->done(value, text);
          if (value)
            g_variant_unref(value);
        },
        new Call{this, std::move(done)});
  }
  void clearPrompt(const char *reason = "Pairing canceled") {
    if (request) {
      g_dbus_method_invocation_return_dbus_error(
          request, "org.bluez.Error.Canceled", reason);
      g_object_unref(request);
      request = nullptr;
    }
    std::lock_guard lock(mutex);
    state["prompt"] = nullptr;
  }
  void updateBusy() {
    std::lock_guard lock(mutex);
    state["busy"] = busy;
    state["pairing"] = pairing;
  }
  void reload() {
    if (owner.empty() || closing)
      return;
    if (refreshing) {
      dirty = true;
      return;
    }
    refreshing = true;
    const auto generation = owner;
    call("/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects",
         nullptr, [this, generation](GVariant *result, const std::string &err) {
           refreshing = false;
           if (owner != generation) {
             reload();
             return;
           }
           if (!err.empty()) {
             error(err);
             return;
           }
           Json adapters = Json::array(), devices = Json::array();
           auto objects = g_variant_get_child_value(result, 0);
           GVariantIter it;
           g_variant_iter_init(&it, objects);
           const char *path;
           GVariant *interfaces;
           while (
               g_variant_iter_next(&it, "{&o@a{sa{sv}}}", &path, &interfaces)) {
             auto a = g_variant_lookup_value(interfaces, "org.bluez.Adapter1",
                                             G_VARIANT_TYPE("a{sv}"));
             auto d = g_variant_lookup_value(interfaces, "org.bluez.Device1",
                                             G_VARIANT_TYPE("a{sv}"));
             auto b = g_variant_lookup_value(interfaces, "org.bluez.Battery1",
                                             G_VARIANT_TYPE("a{sv}"));
             if (a) {
               auto row = properties(a);
               row["path"] = path;
               row["scanning"] = scans.count(path) > 0;
               adapters.push_back(row);
               g_variant_unref(a);
             }
             if (d) {
               auto row = properties(d);
               row["path"] = path;
               if (b)
                 row["battery"] = properties(b).value("Percentage", 0);
               devices.push_back(row);
               g_variant_unref(d);
             }
             if (b)
               g_variant_unref(b);
             g_variant_unref(interfaces);
           }
           g_variant_unref(objects);
           auto less = [](const Json &a, const Json &b) {
             return a.value("Alias", a.value("Address", std::string())) <
                    b.value("Alias", b.value("Address", std::string()));
           };
           // Hot-unplug releases discovery in BlueZ. Forget our matching local
           // deadlines so a removed adapter cannot produce repeated stop
           // errors.
           for (auto it = scans.begin(); it != scans.end();) {
             bool exists =
                 std::any_of(adapters.begin(), adapters.end(),
                             [&](const Json &a) { return a["path"] == *it; });
             if (!exists) {
               deadlines.erase(*it);
               it = scans.erase(it);
             } else
               ++it;
           }
           std::sort(adapters.begin(), adapters.end(), less);
           std::sort(devices.begin(), devices.end(), less);
           {
             std::lock_guard lock(mutex);
             state["available"] = true;
             state["adapters"] = adapters;
             state["devices"] = devices;
           }
           if (dirty) {
             dirty = false;
             reload();
           }
         });
  }
  void registerAgent() {
    const auto generation = owner;
    call("/org/bluez", "org.bluez.AgentManager1", "RegisterAgent",
         g_variant_new("(os)", agentPath, "KeyboardDisplay"),
         [this, generation](GVariant *, const std::string &err) {
           if (owner != generation)
             return;
           if (!err.empty()) {
             error("Pairing agent: " + err);
             return;
           }
           {
             std::lock_guard lock(mutex);
             state["agent"] = true;
           }
           // Incoming pairing still requires explicit user approval in our
           // agent.
           call("/org/bluez", "org.bluez.AgentManager1", "RequestDefaultAgent",
                g_variant_new("(o)", agentPath),
                [this](GVariant *, const std::string &err) {
                  if (!err.empty())
                    error("Incoming pairing unavailable: " + err);
                });
         });
  }
  static void agent(GDBusConnection *, const gchar *sender, const gchar *,
                    const gchar *, const gchar *method, GVariant *args,
                    GDBusMethodInvocation *reply, gpointer data) {
    auto s = static_cast<Impl *>(data);
    if (s->owner != sender) {
      g_dbus_method_invocation_return_dbus_error(
          reply, "org.bluez.Error.Rejected", "Only BlueZ may request pairing");
      return;
    }
    std::string kind = method;
    if (kind == "Cancel" || kind == "Release") {
      s->clearPrompt();
      if (kind == "Release") {
        std::lock_guard lock(s->mutex);
        s->state["agent"] = false;
      }
      g_dbus_method_invocation_return_value(reply, nullptr);
      return;
    }
    bool display = kind == "DisplayPinCode" || kind == "DisplayPasskey";
    if (s->request) {
      g_dbus_method_invocation_return_dbus_error(
          reply, "org.bluez.Error.Rejected",
          "Another pairing request is pending");
      return;
    }
    const char *path = nullptr, *text = "";
    guint32 key = 0;
    guint16 entered = 0;
    if (kind == "DisplayPinCode" || kind == "AuthorizeService")
      g_variant_get(args, "(&o&s)", &path, &text);
    else if (kind == "DisplayPasskey")
      g_variant_get(args, "(&ouq)", &path, &key, &entered);
    else if (kind == "RequestConfirmation")
      g_variant_get(args, "(&ou)", &path, &key);
    else
      g_variant_get(args, "(&o)", &path);
    char code[7];
    snprintf(code, sizeof(code), "%06u", key);
    if (key > 999999) {
      g_dbus_method_invocation_return_dbus_error(
          reply, "org.bluez.Error.Rejected", "Invalid passkey");
      return;
    }
    Json prompt = {{"id", ++s->serial},
                   {"kind", kind},
                   {"device", path},
                   {"code", kind == "DisplayPinCode" ? text : code},
                   {"uuid", kind == "AuthorizeService" ? text : ""},
                   {"entered", entered}};
    {
      std::lock_guard lock(s->mutex);
      s->state["prompt"] = prompt;
    }
    s->requestKind = kind;
    s->promptDeadline = g_get_monotonic_time() + 120 * G_USEC_PER_SEC;
    if (display)
      g_dbus_method_invocation_return_value(reply, nullptr);
    else
      s->request = G_DBUS_METHOD_INVOCATION(g_object_ref(reply));
  }
  void answer(unsigned id, bool accept, const std::string &text) {
    {
      std::lock_guard lock(mutex);
      if (state["prompt"].is_null() || state["prompt"]["id"] != id)
        return;
    }
    if (!request) {
      clearPrompt();
      return;
    }
    if (!accept) {
      g_dbus_method_invocation_return_dbus_error(
          request, "org.bluez.Error.Rejected", "Pairing rejected by user");
    } else if (requestKind == "RequestPinCode") {
      if (text.empty() || text.size() > 16 ||
          !g_utf8_validate(text.c_str(), text.size(), nullptr)) {
        error("Enter a PIN of 1–16 bytes.");
        return;
      }
      g_dbus_method_invocation_return_value(request,
                                            g_variant_new("(s)", text.c_str()));
    } else if (requestKind == "RequestPasskey") {
      if (text.empty() || text.size() > 6 ||
          text.find_first_not_of("0123456789") != std::string::npos) {
        error("Enter a numeric passkey of up to six digits.");
        return;
      }
      g_dbus_method_invocation_return_value(
          request, g_variant_new("(u)", guint32(std::stoul(text))));
    } else
      g_dbus_method_invocation_return_value(request, nullptr);
    g_object_unref(request);
    request = nullptr;
    {
      std::lock_guard lock(mutex);
      state["prompt"] = nullptr;
      state["error"] = "";
    }
  }
  void act(const std::string &path, const std::string &operation) {
    if (!bus || owner.empty()) {
      error("Bluetooth service is unavailable. Start the system Bluetooth "
            "service.");
      return;
    }
    if (operation == "refresh") {
      error("");
      reload();
      return;
    }
    Json row;
    bool adapter = false;
    {
      std::lock_guard lock(mutex);
      for (const auto &a : state["adapters"])
        if (a["path"] == path) {
          row = a;
          adapter = true;
        }
      for (const auto &d : state["devices"])
        if (d["path"] == path)
          row = d;
    }
    if (row.is_null()) {
      error("The selected Bluetooth device is no longer available.");
      return;
    }
    // CancelPairing must remain usable while Pair's asynchronous call is
    // pending.
    if (busy.count(path) && operation != "cancel") {
      error("An operation is already pending for this device.");
      return;
    }
    const char *iface = adapter ? "org.bluez.Adapter1" : "org.bluez.Device1";
    std::string target = path, method;
    GVariant *args = nullptr;
    const std::map<std::string, std::pair<const char *, bool>> props = {
        {"power-on", {"Powered", true}},
        {"power-off", {"Powered", false}},
        {"visible-on", {"Discoverable", true}},
        {"visible-off", {"Discoverable", false}},
        {"pairable-on", {"Pairable", true}},
        {"pairable-off", {"Pairable", false}},
        {"trust", {"Trusted", true}},
        {"untrust", {"Trusted", false}},
        {"block", {"Blocked", true}},
        {"unblock", {"Blocked", false}}};
    auto property = props.find(operation);
    if (property != props.end()) {
      bool forAdapter = operation.rfind("power-", 0) == 0 ||
                        operation.rfind("visible-", 0) == 0 ||
                        operation.rfind("pairable-", 0) == 0;
      if (forAdapter != adapter) {
        error("Invalid Bluetooth action for this selection.");
        return;
      }
      // Keep visibility bounded even if another client changed the timeout.
      if (operation == "visible-on") {
        call(path, "org.freedesktop.DBus.Properties", "Set",
             g_variant_new("(ssv)", iface, "DiscoverableTimeout",
                           g_variant_new_uint32(180)),
             [this, path](GVariant *, const std::string &err) {
               if (!err.empty())
                 error(err);
               else
                 setVisible(path);
             });
        return;
      }
      args = g_variant_new("(ssv)", iface, property->second.first,
                           g_variant_new_boolean(property->second.second));
      iface = "org.freedesktop.DBus.Properties";
      method = "Set";
    } else if (adapter && operation == "scan") {
      if (scans.count(path))
        return;
      method = "StartDiscovery";
    } else if (adapter && operation == "stop") {
      if (!scans.count(path))
        return;
      method = "StopDiscovery";
    } else if (!adapter && operation == "pair")
      method = "Pair";
    else if (!adapter && operation == "cancel")
      method = "CancelPairing";
    else if (!adapter && operation == "connect")
      method = "Connect";
    else if (!adapter && operation == "disconnect")
      method = "Disconnect";
    else if (!adapter && operation == "remove") {
      target = row.value("Adapter", std::string());
      iface = "org.bluez.Adapter1";
      method = "RemoveDevice";
      args = g_variant_new("(o)", path.c_str());
    } else {
      error("Unknown Bluetooth action");
      return;
    }
    if (operation == "pair" && !pairing.empty()) {
      error("Finish or cancel the current pairing first.");
      return;
    }
    if (operation == "pair")
      pairing.insert(path);
    if (operation == "scan")
      allowScanning = true;
    if (operation != "cancel")
      busy.insert(path);
    updateBusy();
    error("");
    const auto generation = owner;
    call(
        target, iface, method.c_str(), args,
        [this, path, operation, generation](GVariant *,
                                            const std::string &err) {
          if (owner != generation)
            return;
          if (operation != "cancel")
            busy.erase(path);
          if (operation == "pair")
            pairing.erase(path);
          updateBusy();
          if (!err.empty())
            error(err);
          if (err.empty() && operation == "scan") {
            scans.insert(path);
            deadlines[path] = g_get_monotonic_time() + 60 * G_USEC_PER_SEC;
          }
          if (err.empty() &&
              (operation == "stop" || operation == "power-off")) {
            scans.erase(path);
            deadlines.erase(path);
          }
          if (operation == "pair") {
            bool matches = false;
            {
              std::lock_guard lock(mutex);
              matches = !state["prompt"].is_null() &&
                        state["prompt"]["device"] == path;
            }
            if (matches)
              clearPrompt();
          }
          if (operation == "scan" && !allowScanning && err.empty())
            act(path, "stop");
          reload();
        },
        operation == "pair" ? 120000 : 15000);
  }
  void setVisible(const std::string &path) {
    call(path, "org.freedesktop.DBus.Properties", "Set",
         g_variant_new("(ssv)", "org.bluez.Adapter1", "Discoverable",
                       g_variant_new_boolean(TRUE)),
         [this](GVariant *, const std::string &err) {
           if (!err.empty())
             error(err);
           reload();
         });
  }
  void cancelPairs() {
    auto copy = pairing;
    for (const auto &path : copy)
      act(path, "cancel");
    clearPrompt("Pairing canceled by user");
  }
  void stopScans() {
    allowScanning = false;
    auto copy = scans;
    for (const auto &path : copy)
      act(path, "stop");
  }
  Impl() {
    worker = std::thread([this] {
      g_main_context_push_thread_default(context);
      GError *err = nullptr;
      auto address =
          g_dbus_address_get_for_bus_sync(G_BUS_TYPE_SYSTEM, cancel, &err);
      if (address) {
        bus = g_dbus_connection_new_for_address_sync(
            address,
            GDBusConnectionFlags(
                G_DBUS_CONNECTION_FLAGS_AUTHENTICATION_CLIENT |
                G_DBUS_CONNECTION_FLAGS_MESSAGE_BUS_CONNECTION),
            nullptr, cancel, &err);
        g_free(address);
      }
      if (!bus) {
        error(err ? err->message : "System bus unavailable");
        g_clear_error(&err);
      } else {
        info = g_dbus_node_info_new_for_xml(agentXml, nullptr);
        static const GDBusInterfaceVTable vtable = {
            agent, nullptr, nullptr, {nullptr}};
        object = g_dbus_connection_register_object(
            bus, agentPath, info->interfaces[0], &vtable, this, nullptr, &err);
        if (!object) {
          error(err->message);
          g_clear_error(&err);
        }
        signals = g_dbus_connection_signal_subscribe(
            bus, "org.bluez", nullptr, nullptr, nullptr, nullptr,
            G_DBUS_SIGNAL_FLAGS_NONE,
            [](GDBusConnection *, const gchar *, const gchar *,
               const gchar *iface, const gchar *name, GVariant *, gpointer p) {
              if ((!strcmp(iface, "org.freedesktop.DBus.ObjectManager") &&
                   (!strcmp(name, "InterfacesAdded") ||
                    !strcmp(name, "InterfacesRemoved"))) ||
                  (!strcmp(iface, "org.freedesktop.DBus.Properties") &&
                   !strcmp(name, "PropertiesChanged")))
                static_cast<Impl *>(p)->reload();
            },
            this, nullptr);
        watch = g_bus_watch_name_on_connection(
            bus, "org.bluez", G_BUS_NAME_WATCHER_FLAGS_NONE,
            [](GDBusConnection *, const gchar *, const gchar *name,
               gpointer p) {
              auto s = static_cast<Impl *>(p);
              s->owner = name;
              s->error("");
              s->reload();
              if (s->object)
                s->registerAgent();
            },
            [](GDBusConnection *, const gchar *, gpointer p) {
              auto s = static_cast<Impl *>(p);
              s->owner.clear();
              s->scans.clear();
              s->deadlines.clear();
              s->busy.clear();
              s->pairing.clear();
              s->clearPrompt();
              std::lock_guard lock(s->mutex);
              s->state["available"] = false;
              s->state["agent"] = false;
              s->state["busy"] = Json::array();
              s->state["pairing"] = Json::array();
              s->state["adapters"] = Json::array();
              s->state["devices"] = Json::array();
              s->state["error"] = "Bluetooth service is unavailable. Start the "
                                  "system Bluetooth service.";
            },
            this, nullptr);
      }
      timer = g_timeout_source_new_seconds(1);
      g_source_set_callback(
          timer,
          [](gpointer p) -> gboolean {
            auto s = static_cast<Impl *>(p);
            auto now = g_get_monotonic_time();
            auto deadlines = s->deadlines;
            for (auto &[path, end] : deadlines)
              if (now >= end)
                s->act(path, "stop");
            if (s->promptDeadline && now >= s->promptDeadline) {
              s->clearPrompt("Pairing request timed out");
              s->promptDeadline = 0;
            }
            return G_SOURCE_CONTINUE;
          },
          this, nullptr);
      g_source_attach(timer, context);
      g_main_loop_run(loop);
      closing = true;
      clearPrompt("Bluetooth manager closed");
      g_cancellable_cancel(cancel);
      while (inflight)
        g_main_context_iteration(context, TRUE);
      if (watch)
        g_bus_unwatch_name(watch);
      if (signals)
        g_dbus_connection_signal_unsubscribe(bus, signals);
      if (object)
        g_dbus_connection_unregister_object(bus, object);
      g_source_destroy(timer);
      g_source_unref(timer);
      if (info)
        g_dbus_node_info_unref(info);
      // Disconnecting our private bus connection releases this agent and only
      // our discovery sessions, even if a scan was still starting at shutdown.
      if (bus) {
        g_dbus_connection_close_sync(bus, nullptr, nullptr);
        g_object_unref(bus);
      }
      g_main_context_pop_thread_default(context);
    });
  }
  ~Impl() {
    post([this] {
      stopScans();
      g_main_loop_quit(loop);
    });
    worker.join();
    g_object_unref(cancel);
    g_main_loop_unref(loop);
    g_main_context_unref(context);
  }
};
Client::Client() : impl(std::make_unique<Impl>()) {}
Client::~Client() = default;
Json Client::snapshot() const {
  std::lock_guard lock(impl->mutex);
  return impl->state;
}
void Client::refresh() {
  impl->post([s = impl.get()] { s->act("", "refresh"); });
}
void Client::action(const std::string &path, const std::string &op) {
  impl->post([s = impl.get(), path, op] { s->act(path, op); });
}
void Client::respond(unsigned id, bool accept, const std::string &value) {
  impl->post(
      [s = impl.get(), id, accept, value] { s->answer(id, accept, value); });
}
void Client::cancelPairing() {
  impl->post([s = impl.get()] { s->cancelPairs(); });
}
void Client::stopDiscovery() {
  impl->post([s = impl.get()] { s->stopScans(); });
}
} // namespace bluetooth
