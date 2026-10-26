// SPDX-License-Identifier: MIT
#include "network.h"
#include <algorithm>
#include <arpa/inet.h>
#include <memory>
#include <sstream>
#include <stdexcept>
namespace network {
namespace {
[[noreturn]] void fail(const std::string &s) { throw std::runtime_error(s); }
std::string text(const char *s) { return s ? s : ""; }
template <class T> struct Unref {
  void operator()(T *p) const {
    if (p)
      g_object_unref(p);
  }
};
template <class T> using Object = std::unique_ptr<T, Unref<T>>;
struct Session {
  GMainContext *context = g_main_context_new();
  NMClient *client = nullptr;
  Session() {
    g_main_context_push_thread_default(context);
    GError *error = nullptr;
    client = nm_client_new(nullptr, &error);
    if (!client || !nm_client_get_nm_running(client)) {
      std::string message = error ? error->message : "NetworkManager is not running";
      g_clear_error(&error);
      if (client)
        g_object_unref(client);
      g_main_context_pop_thread_default(context);
      g_main_context_unref(context);
      fail(message);
    }
  }
  ~Session() {
    g_object_unref(client);
    while (g_main_context_pending(context))
      g_main_context_iteration(context, FALSE);
    g_main_context_pop_thread_default(context);
    g_main_context_unref(context);
  }
  template <class Start, class Finish> void wait(Start start, Finish finish) {
    struct Pending {
      GAsyncResult *result = nullptr;
      GCancellable *cancel;
    } pending{nullptr, g_cancellable_new()};
    auto timeout = g_timeout_source_new_seconds(30);
    g_source_set_callback(
        timeout,
        [](gpointer p) -> gboolean {
          g_cancellable_cancel(static_cast<Pending *>(p)->cancel);
          return G_SOURCE_REMOVE;
        },
        &pending, nullptr);
    g_source_attach(timeout, context);
    start(
        pending.cancel,
        +[](GObject *, GAsyncResult *result, gpointer data) {
          static_cast<Pending *>(data)->result = G_ASYNC_RESULT(g_object_ref(result));
        },
        &pending);
    while (!pending.result)
      g_main_context_iteration(context, TRUE);
    g_source_destroy(timeout);
    g_source_unref(timeout);
    g_object_unref(pending.cancel);
    GError *error = nullptr;
    finish(pending.result, &error);
    g_object_unref(pending.result);
    if (error) {
      std::string message = error->message;
      g_error_free(error);
      fail(message);
    }
  }
};
std::string state(NMDeviceState value) {
  switch (value) {
  case NM_DEVICE_STATE_ACTIVATED:
    return "Connected";
  case NM_DEVICE_STATE_DISCONNECTED:
    return "Disconnected";
  case NM_DEVICE_STATE_UNAVAILABLE:
    return "Unavailable";
  case NM_DEVICE_STATE_UNMANAGED:
    return "Unmanaged";
  case NM_DEVICE_STATE_FAILED:
    return "Failed";
  default:
    return "Connecting / configuring";
  }
}
std::string bytesHex(GBytes *bytes) {
  if (!bytes)
    return {};
  gsize length = 0;
  auto data = static_cast<const unsigned char *>(g_bytes_get_data(bytes, &length));
  const char *hex = "0123456789abcdef";
  std::string result;
  for (gsize i = 0; i < length; ++i) {
    result += hex[data[i] >> 4];
    result += hex[data[i] & 15];
  }
  return result;
}
std::string decode(const std::string &hex) {
  if (hex.empty() || hex.size() > 64 || hex.size() % 2)
    fail("SSID must contain 1–32 bytes");
  std::string result;
  auto digit = [](char c) -> int {
    if (c >= '0' && c <= '9')
      return c - '0';
    if (c >= 'a' && c <= 'f')
      return c - 'a' + 10;
    if (c >= 'A' && c <= 'F')
      return c - 'A' + 10;
    return -1;
  };
  for (size_t i = 0; i < hex.size(); i += 2) {
    int a = digit(hex[i]), b = digit(hex[i + 1]);
    if (a < 0 || b < 0)
      fail("Invalid SSID");
    result += char((a << 4) | b);
  }
  return result;
}
std::string ssidName(GBytes *bytes) {
  if (!bytes)
    return "Hidden network";
  gsize size = 0;
  auto data = static_cast<const guint8 *>(g_bytes_get_data(bytes, &size));
  gchar *name = nm_utils_ssid_to_utf8(data, size);
  std::string result = text(name);
  g_free(name);
  return result;
}
NMRemoteConnection *connection(Session &s, const Json &r) {
  auto uuid = r.at("uuid").get<std::string>();
  auto c = nm_client_get_connection_by_uuid(s.client, uuid.c_str());
  if (!c)
    fail("The connection profile is no longer available");
  return c;
}
std::string joinDns(NMSettingIPConfig *ip) {
  std::string result;
  if (ip)
    for (guint i = 0; i < nm_setting_ip_config_get_num_dns(ip); ++i) {
      if (i)
        result += ", ";
      result += nm_setting_ip_config_get_dns(ip, i);
    }
  return result;
}
Json ipSettings(NMSettingIPConfig *ip) {
  Json result = {{"method", ip ? text(nm_setting_ip_config_get_method(ip)) : "auto"},
                 {"dns", joinDns(ip)},
                 {"address", ""},
                 {"gateway", ip ? text(nm_setting_ip_config_get_gateway(ip)) : ""},
                 {"ignore_auto_dns", ip && nm_setting_ip_config_get_ignore_auto_dns(ip)}};
  if (ip && nm_setting_ip_config_get_num_addresses(ip)) {
    auto a = nm_setting_ip_config_get_address(ip, 0);
    result["address"] =
        text(nm_ip_address_get_address(a)) + "/" + std::to_string(nm_ip_address_get_prefix(a));
  }
  return result;
}
void verify(NMConnection *c) {
  GError *error = nullptr;
  if (!nm_connection_verify(c, &error)) {
    std::string message = error ? error->message : "Invalid connection settings";
    g_clear_error(&error);
    fail(message);
  }
}
void addIp(NMConnection *c) {
  auto ip4 = nm_setting_ip4_config_new();
  g_object_set(ip4, NM_SETTING_IP_CONFIG_METHOD, "auto", nullptr);
  nm_connection_add_setting(c, ip4);
  auto ip6 = nm_setting_ip6_config_new();
  g_object_set(ip6, NM_SETTING_IP_CONFIG_METHOD, "auto", nullptr);
  nm_connection_add_setting(c, ip6);
}
void basic(NMConnection *c, const std::string &name, const char *type) {
  if (name.empty() || !g_utf8_validate(name.c_str(), name.size(), nullptr))
    fail("Enter a valid connection name");
  auto setting = NM_SETTING_CONNECTION(nm_setting_connection_new());
  auto uuid = g_uuid_string_random();
  g_object_set(setting, NM_SETTING_CONNECTION_ID, name.c_str(), NM_SETTING_CONNECTION_UUID, uuid,
               NM_SETTING_CONNECTION_TYPE, type, NM_SETTING_CONNECTION_AUTOCONNECT, TRUE, nullptr);
  g_free(uuid);
  nm_setting_connection_add_permission(setting, "user", g_get_user_name(), nullptr);
  nm_connection_add_setting(c, NM_SETTING(setting));
  addIp(c);
}
void updateIp(NMConnection *c, const Json &fields, int family) {
  auto ip = family == AF_INET ? nm_connection_get_setting_ip4_config(c)
                              : nm_connection_get_setting_ip6_config(c);
  if (!ip) {
    auto created = family == AF_INET ? nm_setting_ip4_config_new() : nm_setting_ip6_config_new();
    nm_connection_add_setting(c, created);
    ip = NM_SETTING_IP_CONFIG(created);
  }
  auto method = fields.at("method").get<std::string>();
  if (method != "auto" && method != "manual" && method != "disabled" && method != "link-local" &&
      method != "shared" && !(family == AF_INET6 && (method == "ignore" || method == "dhcp")))
    fail("Unsupported IP method");
  g_object_set(ip, NM_SETTING_IP_CONFIG_METHOD, method.c_str(),
               NM_SETTING_IP_CONFIG_IGNORE_AUTO_DNS, fields.value("ignore_auto_dns", false),
               nullptr);
  auto address = fields.value("address", std::string());
  // Only replace addresses when the editor changes them; additional addresses and routes survive.
  auto before = ipSettings(ip).value("address", std::string());
  if (address != before) {
    nm_setting_ip_config_clear_addresses(ip);
    if (!address.empty()) {
      auto slash = address.find('/');
      if (slash == std::string::npos)
        fail("IP address must include its prefix, such as 192.168.1.5/24");
      auto prefix = address.substr(slash + 1);
      if (prefix.empty() || prefix.find_first_not_of("0123456789") != std::string::npos)
        fail("Invalid address prefix");
      auto n = std::stoul(prefix);
      if (n > (family == AF_INET ? 32u : 128u))
        fail("Invalid address prefix");
      GError *error = nullptr;
      auto value = nm_ip_address_new(family, address.substr(0, slash).c_str(), n, &error);
      if (!value) {
        std::string message = error ? error->message : "Invalid IP address";
        g_clear_error(&error);
        fail(message);
      }
      nm_setting_ip_config_add_address(ip, value);
      nm_ip_address_unref(value);
    }
  }
  auto gateway = fields.value("gateway", std::string());
  if (!gateway.empty() && !nm_utils_ipaddr_valid(family, gateway.c_str()))
    fail("Invalid gateway address");
  g_object_set(ip, NM_SETTING_IP_CONFIG_GATEWAY, gateway.empty() ? nullptr : gateway.c_str(),
               nullptr);
  auto dns = fields.value("dns", std::string());
  std::replace(dns.begin(), dns.end(), ',', ' ');
  std::istringstream stream(dns);
  std::string server;
  nm_setting_ip_config_clear_dns(ip);
  while (stream >> server) {
    if (!nm_utils_ipaddr_valid(family, server.c_str()))
      fail("Invalid DNS server address");
    nm_setting_ip_config_add_dns(ip, server.c_str());
  }
}
} // namespace
NMConnection *wifiConnection(const Json &r) {
  Object<NMConnection> c(nm_simple_connection_new());
  auto ssid = decode(r.at("ssid").get<std::string>());
  basic(c.get(), r.value("name", ssid), NM_SETTING_WIRELESS_SETTING_NAME);
  auto wifi = NM_SETTING_WIRELESS(nm_setting_wireless_new());
  auto bytes = g_bytes_new(ssid.data(), ssid.size());
  g_object_set(wifi, NM_SETTING_WIRELESS_SSID, bytes, NM_SETTING_WIRELESS_MODE, "infrastructure",
               NM_SETTING_WIRELESS_HIDDEN, r.value("hidden", false), nullptr);
  g_bytes_unref(bytes);
  nm_connection_add_setting(c.get(), NM_SETTING(wifi));
  auto security = r.value("security", std::string("open"));
  if (security != "open") {
    if (security != "wpa-psk" && security != "sae")
      fail("Enterprise Wi-Fi requires an existing configured connection profile");
    auto password = r.value("password", std::string());
    if (security == "sae"
            ? (password.empty() || password.size() > 63)
            : !((password.size() >= 8 && password.size() <= 63) ||
                (password.size() == 64 &&
                 password.find_first_not_of("0123456789abcdefABCDEF") == std::string::npos)))
      fail("Enter a valid Wi-Fi password");
    auto sec = nm_setting_wireless_security_new();
    g_object_set(sec, NM_SETTING_WIRELESS_SECURITY_KEY_MGMT, security.c_str(),
                 NM_SETTING_WIRELESS_SECURITY_PSK, password.c_str(), nullptr);
    nm_connection_add_setting(c.get(), sec);
  }
  verify(c.get());
  return c.release();
}
void editProfile(NMConnection *c, const Json &r) {
  auto setting = nm_connection_get_setting_connection(c);
  if (!setting)
    fail("Invalid connection profile");
  auto name = r.at("name").get<std::string>();
  if (name.empty() || !g_utf8_validate(name.c_str(), name.size(), nullptr))
    fail("Enter a valid connection name");
  g_object_set(setting, NM_SETTING_CONNECTION_ID, name.c_str(), NM_SETTING_CONNECTION_AUTOCONNECT,
               r.value("autoconnect", true), nullptr);
  if (r.contains("ipv4"))
    updateIp(c, r.at("ipv4"), AF_INET);
  if (r.contains("ipv6"))
    updateIp(c, r.at("ipv6"), AF_INET6);
  verify(c);
}
Json profile(const std::string &uuid) {
  Session s;
  auto c = connection(s, {{"uuid", uuid}});
  auto basic = nm_connection_get_setting_connection(NM_CONNECTION(c));
  return {{"uuid", uuid},
          {"name", text(nm_connection_get_id(NM_CONNECTION(c)))},
          {"autoconnect", bool(nm_setting_connection_get_autoconnect(basic))},
          {"ipv4", ipSettings(nm_connection_get_setting_ip4_config(NM_CONNECTION(c)))},
          {"ipv6", ipSettings(nm_connection_get_setting_ip6_config(NM_CONNECTION(c)))}};
}
Json snapshot() {
  Session s;
  Json result = {{"enabled", bool(nm_client_networking_get_enabled(s.client))},
                 {"wireless", bool(nm_client_wireless_get_enabled(s.client))},
                 {"hardware_wireless", bool(nm_client_wireless_hardware_get_enabled(s.client))},
                 {"devices", Json::array()},
                 {"networks", Json::array()},
                 {"profiles", Json::array()}};
  auto devices = nm_client_get_devices(s.client);
  for (guint i = 0; i < devices->len; ++i) {
    auto d = NM_DEVICE(g_ptr_array_index(devices, i));
    auto type = nm_device_get_device_type(d);
    auto active = nm_device_get_active_connection(d);
    Json row = {{"path", text(nm_object_get_path(NM_OBJECT(d)))},
                {"interface", text(nm_device_get_iface(d))},
                {"type", int(type)},
                {"state", state(nm_device_get_state(d))},
                {"connection", active ? text(nm_active_connection_get_id(active)) : ""},
                {"addresses", Json::array()},
                {"dns", Json::array()}};
    for (auto ip : {nm_device_get_ip4_config(d), nm_device_get_ip6_config(d)})
      if (ip) {
        auto addresses = nm_ip_config_get_addresses(ip);
        for (guint n = 0; n < addresses->len; ++n) {
          auto address = static_cast<NMIPAddress *>(g_ptr_array_index(addresses, n));
          row["addresses"].push_back(text(nm_ip_address_get_address(address)) + "/" +
                                     std::to_string(nm_ip_address_get_prefix(address)));
        }
        auto servers = nm_ip_config_get_nameservers(ip);
        if (servers)
          for (int n = 0; servers[n]; ++n)
            row["dns"].push_back(servers[n]);
      }
    result["devices"].push_back(row);
    if (type != NM_DEVICE_TYPE_WIFI)
      continue;
    auto points = nm_device_wifi_get_access_points(NM_DEVICE_WIFI(d));
    auto selected = nm_device_wifi_get_active_access_point(NM_DEVICE_WIFI(d));
    for (guint n = 0; n < points->len; ++n) {
      auto ap = NM_ACCESS_POINT(g_ptr_array_index(points, n));
      auto flags = nm_access_point_get_rsn_flags(ap) | nm_access_point_get_wpa_flags(ap);
      std::string security = (flags & NM_802_11_AP_SEC_KEY_MGMT_PSK)      ? "wpa-psk"
                             : (flags & NM_802_11_AP_SEC_KEY_MGMT_SAE)    ? "sae"
                             : (flags & NM_802_11_AP_SEC_KEY_MGMT_802_1X) ? "enterprise"
                             : (nm_access_point_get_flags(ap) & NM_802_11_AP_FLAGS_PRIVACY)
                                 ? "legacy"
                                 : "open";
      result["networks"].push_back({{"path", text(nm_object_get_path(NM_OBJECT(ap)))},
                                    {"device", row["path"]},
                                    {"ssid", bytesHex(nm_access_point_get_ssid(ap))},
                                    {"name", ssidName(nm_access_point_get_ssid(ap))},
                                    {"security", security},
                                    {"signal", nm_access_point_get_strength(ap)},
                                    {"active", ap == selected}});
    }
  }
  auto profiles = nm_client_get_connections(s.client);
  auto active = nm_client_get_active_connections(s.client);
  for (guint i = 0; i < profiles->len; ++i) {
    auto c = NM_CONNECTION(g_ptr_array_index(profiles, i));
    bool connected = false;
    for (guint n = 0; n < active->len; ++n)
      if (text(nm_active_connection_get_uuid(NM_ACTIVE_CONNECTION(g_ptr_array_index(active, n)))) ==
          text(nm_connection_get_uuid(c)))
        connected = true;
    result["profiles"].push_back({{"uuid", text(nm_connection_get_uuid(c))},
                                  {"name", text(nm_connection_get_id(c))},
                                  {"type", text(nm_connection_get_connection_type(c))},
                                  {"active", connected}});
  }
  std::sort(result["networks"].begin(), result["networks"].end(), [](const Json &a, const Json &b) {
    return a["signal"].get<int>() > b["signal"].get<int>();
  });
  return result;
}
void action(const Json &r) {
  Session s;
  auto operation = r.at("action").get<std::string>();
  if (operation == "wireless" || operation == "enabled") {
    GError *error = nullptr;
    auto bus = nm_client_get_dbus_connection(s.client);
    auto arguments =
        operation == "wireless"
            ? g_variant_new("(ssv)", "org.freedesktop.NetworkManager", "WirelessEnabled",
                            g_variant_new_boolean(r.at("enabled").get<bool>()))
            : g_variant_new("(b)", r.at("enabled").get<bool>());
    auto reply = g_dbus_connection_call_sync(
        bus, "org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager",
        operation == "wireless" ? "org.freedesktop.DBus.Properties"
                                : "org.freedesktop.NetworkManager",
        operation == "wireless" ? "Set" : "Enable", arguments, nullptr,
        G_DBUS_CALL_FLAGS_ALLOW_INTERACTIVE_AUTHORIZATION, 30000, nullptr, &error);
    if (!reply) {
      std::string message = error ? error->message : "Cannot change networking";
      g_clear_error(&error);
      fail(message);
    }
    g_variant_unref(reply);
    return;
  }
  if (operation == "activate") {
    auto c = connection(s, r);
    s.wait(
        [&](auto cancel, auto callback, auto data) {
          nm_client_activate_connection_async(s.client, NM_CONNECTION(c), nullptr, nullptr, cancel,
                                              callback, data);
        },
        [&](auto result, auto error) {
          auto active = nm_client_activate_connection_finish(s.client, result, error);
          if (active)
            g_object_unref(active);
        });
    return;
  }
  if (operation == "deactivate") {
    auto uuid = r.at("uuid").get<std::string>();
    auto list = nm_client_get_active_connections(s.client);
    NMActiveConnection *active = nullptr;
    for (guint i = 0; i < list->len; ++i) {
      auto c = NM_ACTIVE_CONNECTION(g_ptr_array_index(list, i));
      if (text(nm_active_connection_get_uuid(c)) == uuid)
        active = c;
    }
    if (!active)
      fail("This connection is not active");
    s.wait(
        [&](auto cancel, auto callback, auto data) {
          nm_client_deactivate_connection_async(s.client, active, cancel, callback, data);
        },
        [&](auto result, auto error) {
          nm_client_deactivate_connection_finish(s.client, result, error);
        });
    return;
  }
  if (operation == "delete") {
    auto c = connection(s, r);
    s.wait([&](auto cancel, auto callback,
               auto data) { nm_remote_connection_delete_async(c, cancel, callback, data); },
           [&](auto result, auto error) { nm_remote_connection_delete_finish(c, result, error); });
    return;
  }
  if (operation == "edit") {
    auto c = connection(s, r);
    Object<NMConnection> copy(nm_simple_connection_new_clone(NM_CONNECTION(c)));
    editProfile(copy.get(), r);
    std::unique_ptr<GVariant, decltype(&g_variant_unref)> settings(
        // Omitted secrets are merged by NetworkManager; never read them into the editor.
        g_variant_ref_sink(nm_connection_to_dbus(copy.get(), NM_CONNECTION_SERIALIZE_NO_SECRETS)),
        g_variant_unref);
    s.wait(
        [&](auto cancel, auto callback, auto data) {
          nm_remote_connection_update2(c, settings.get(), NM_SETTINGS_UPDATE2_FLAG_TO_DISK, nullptr,
                                       cancel, callback, data);
        },
        [&](auto result, auto error) {
          auto value = nm_remote_connection_update2_finish(c, result, error);
          if (value)
            g_variant_unref(value);
        });
    return;
  }
  auto path = r.at("device").get<std::string>();
  auto d = nm_client_get_device_by_path(s.client, path.c_str());
  if (!d)
    fail("The network device was removed");
  if (operation == "scan") {
    if (!NM_IS_DEVICE_WIFI(d))
      fail("Select a Wi-Fi adapter");
    s.wait(
        [&](auto cancel, auto callback, auto data) {
          nm_device_wifi_request_scan_async(NM_DEVICE_WIFI(d), cancel, callback, data);
        },
        [&](auto result, auto error) {
          nm_device_wifi_request_scan_finish(NM_DEVICE_WIFI(d), result, error);
        });
    return;
  }
  if (operation == "disconnect") {
    s.wait([&](auto cancel, auto callback,
               auto data) { nm_device_disconnect_async(d, cancel, callback, data); },
           [&](auto result, auto error) { nm_device_disconnect_finish(d, result, error); });
    return;
  }
  if (operation == "wifi" || operation == "wired") {
    Object<NMConnection> c;
    std::string point = r.value("path", std::string());
    if (operation == "wifi") {
      if (!NM_IS_DEVICE_WIFI(d))
        fail("Select a Wi-Fi adapter");
      if (!point.empty()) {
        auto ap = nm_device_wifi_get_access_point_by_path(NM_DEVICE_WIFI(d), point.c_str());
        if (!ap)
          fail("That access point is no longer available");
        if (r.value("password", std::string()).empty()) {
          auto profiles = nm_device_get_available_connections(d);
          for (guint i = 0; i < profiles->len; ++i) {
            auto saved = NM_CONNECTION(g_ptr_array_index(profiles, i));
            if (nm_access_point_connection_valid(ap, saved)) {
              s.wait(
                  [&](auto cancel, auto callback, auto data) {
                    nm_client_activate_connection_async(s.client, saved, d, point.c_str(), cancel,
                                                        callback, data);
                  },
                  [&](auto result, auto error) {
                    auto active = nm_client_activate_connection_finish(s.client, result, error);
                    if (active)
                      g_object_unref(active);
                  });
              return;
            }
          }
        }
      }
      c.reset(wifiConnection(r));
    } else {
      if (nm_device_get_device_type(d) != NM_DEVICE_TYPE_ETHERNET)
        fail("Select a wired Ethernet device");
      c.reset(nm_simple_connection_new());
      basic(c.get(), text(nm_device_get_iface(d)) + " wired", NM_SETTING_WIRED_SETTING_NAME);
      nm_connection_add_setting(c.get(), nm_setting_wired_new());
    }
    s.wait(
        [&](auto cancel, auto callback, auto data) {
          nm_client_add_and_activate_connection_async(s.client, c.get(), d,
                                                      point.empty() ? nullptr : point.c_str(),
                                                      cancel, callback, data);
        },
        [&](auto result, auto error) {
          auto active = nm_client_add_and_activate_connection_finish(s.client, result, error);
          if (active)
            g_object_unref(active);
        });
    return;
  }
  fail("Unknown network action");
}
} // namespace network
