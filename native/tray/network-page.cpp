// SPDX-License-Identifier: MIT
#include "network-page.h"
#include "gpu-window.h"
#include "process.h"
#include <FL/Fl_Check_Button.H>
#include <FL/Fl_Choice.H>
#include <FL/Fl_Input.H>
#include <FL/Fl_Native_File_Chooser.H>
#include <FL/Fl_Return_Button.H>
#include <FL/Fl_Secret_Input.H>
#include <FL/fl_ask.H>
#include <chrono>
using Json = network::Json;
namespace {
double now() {
  return std::chrono::duration<double>(std::chrono::steady_clock::now().time_since_epoch()).count();
}
struct Modal {
  bool &value;
  Modal(bool &v) : value(v) { value = true; }
  ~Modal() { value = false; }
};
} // namespace
void NetworkPage::on(Fl_Widget *w, std::function<void()> f) {
  callbacks.push_back([this, f = std::move(f)] {
    try {
      f();
    } catch (const std::exception &e) {
      report(e.what());
    }
  });
  w->callback([](Fl_Widget *, void *p) { (*static_cast<std::function<void()> *>(p))(); },
              &callbacks.back());
}
Fl_Button *NetworkPage::button(int x, int y, int w, const char *label, std::function<void()> fn) {
  auto b = new Fl_Button(x, y, w, 30, label);
  on(b, std::move(fn));
  return b;
}
Json NetworkPage::selected(const char *kind) const {
  auto list = std::string(kind) == "devices"    ? devices
              : std::string(kind) == "networks" ? networks
                                                : profiles;
  int i = list->value() - 1;
  if (!state.contains(kind) || i < 0 || i >= int(state[kind].size()))
    return Json();
  return state[kind][i];
}
void NetworkPage::submit(std::function<Json()> work) {
  if (pending.valid()) {
    report("Wait for the network operation.");
    return;
  }
  report("Network operation in progress…");
  pending = std::async(std::launch::async, std::move(work));
}
void NetworkPage::refresh() {
  if (!pending.valid())
    submit(network::snapshot);
}
void NetworkPage::apply(Json request) {
  submit([request = std::move(request)] {
    network::action(request);
    return network::snapshot();
  });
}
void NetworkPage::update(Json data) {
  auto securityLabel = [](const std::string &value) {
    return value == "open"         ? "Open"
           : value == "wpa-psk"    ? "WPA Personal"
           : value == "sae"        ? "WPA3 Personal"
           : value == "enterprise" ? "Enterprise"
                                   : "Legacy security";
  };
  auto typeLabel = [](const std::string &value) {
    return value == NM_SETTING_WIRELESS_SETTING_NAME ? "Wi-Fi"
           : value == NM_SETTING_WIRED_SETTING_NAME  ? "Ethernet"
           : value == "vpn"                          ? "VPN"
           : value == "wireguard"                    ? "WireGuard"
                                                     : value.c_str();
  };
  auto d = selected("devices"), n = selected("networks"), p = selected("profiles");
  state = std::move(data);
  devices->clear();
  networks->clear();
  profiles->clear();
  for (auto &row : state["devices"]) {
    devices->add(
        (row["interface"].get<std::string>() + " — " + row["state"].get<std::string>()).c_str());
    if (!d.is_null() && row["path"] == d["path"])
      devices->value(devices->size());
  }
  for (auto &row : state["networks"]) {
    networks->add(((row["active"].get<bool>() ? "● " : "") + row["name"].get<std::string>() +
                   " · " + std::string(securityLabel(row["security"].get<std::string>())) + " · " +
                   std::to_string(row["signal"].get<int>()) + "%")
                      .c_str());
    if (!n.is_null() && row["path"] == n["path"])
      networks->value(networks->size());
  }
  for (auto &row : state["profiles"]) {
    profiles->add(((row["active"].get<bool>() ? "● " : "") + row["name"].get<std::string>() +
                   " · " + typeLabel(row["type"].get<std::string>()))
                      .c_str());
    if (!p.is_null() && row["uuid"] == p["uuid"])
      profiles->value(profiles->size());
  }
  wireless->copy_label(state.value("wireless", false) ? "Turn Wi-Fi off" : "Turn Wi-Fi on");
  networking->copy_label(state.value("enabled", false) ? "Disable networking"
                                                       : "Enable networking");
  std::string details =
      state.value("hardware_wireless", true) ? "" : "Wi-Fi is blocked by a hardware switch.\n";
  for (const auto &row : state["devices"]) {
    details +=
        row["interface"].get<std::string>() + ": " + row["connection"].get<std::string>() + "\n";
    for (const auto &a : row["addresses"])
      details += "  " + a.get<std::string>() + "\n";
    for (const auto &a : row["dns"])
      details += "  DNS " + a.get<std::string>() + "\n";
  }
  info->value(details.c_str());
}
void NetworkPage::tick() {
  if (pending.valid() && pending.wait_for(std::chrono::seconds(0)) == std::future_status::ready) {
    try {
      auto result = pending.get();
      if (result.contains("editor")) {
        editor(result.at("editor"));
      } else {
        update(std::move(result));
        report("Network status updated");
      }
    } catch (const std::exception &e) {
      report(e.what());
    }
    nextRefresh = now() + 4;
  }
  if (visible_r() && !modalOpen && !pending.valid() && now() > nextRefresh) {
    nextRefresh = now() + 4;
    refresh();
  }
}
void NetworkPage::connectWifi(bool hidden) {
  if (pending.valid()) {
    report("Wait for the network operation.");
    return;
  }
  Modal modal(modalOpen);
  Json row = selected("networks");
  if (row.is_null() && !hidden) {
    report("Select a Wi-Fi network.");
    return;
  }
  auto device = hidden ? selected("devices") : Json{{"path", row["device"]}};
  if (device.is_null()) {
    report("Select a Wi-Fi adapter.");
    return;
  }
  dx::GpuWindow dialog(560, 240, hidden ? "Hidden Wi-Fi network" : "Connect Wi-Fi");
  Fl_Input ssid(130, 20, 400, 32, "Network:");
  ssid.value(hidden ? "" : row.value("name", std::string()).c_str());
  if (!hidden)
    ssid.deactivate();
  Fl_Choice security(130, 65, 400, 32, "Security:");
  security.add("Open|WPA2 Personal|WPA3 Personal");
  security.value(1);
  std::string kind = hidden ? "wpa-psk" : row.value("security", std::string());
  if (!hidden) {
    security.value(kind == "open" ? 0 : kind == "sae" ? 2 : 1);
    security.deactivate();
  }
  Fl_Secret_Input secret(130, 110, 400, 32, "Password:");
  secret.tooltip("Leave empty to use an existing saved profile. New personal-network passwords are "
                 "stored by NetworkManager.");
  Fl_Button cancel(280, 185, 110, 32, "Cancel");
  Fl_Return_Button connect(405, 185, 125, 32, "Connect");
  bool accepted = false;
  struct Result {
    Fl_Window *window;
    bool *accepted;
  } result{&dialog, &accepted};
  connect.callback(
      [](Fl_Widget *, void *p) {
        auto r = static_cast<Result *>(p);
        *r->accepted = true;
        r->window->hide();
      },
      &result);
  cancel.callback([](Fl_Widget *, void *p) { static_cast<Fl_Window *>(p)->hide(); }, &dialog);
  dialog.end();
  dialog.set_modal();
  dx::style(&dialog);
  dialog.show();
  while (dialog.shown())
    Fl::wait();
  if (accepted) {
    Json request = {{"action", "wifi"},
                    {"device", device["path"]},
                    {"name", std::string(ssid.value())},
                    {"hidden", hidden},
                    {"password", std::string(secret.value())}};
    if (hidden) {
      auto bytes = std::string(ssid.value());
      const char *hex = "0123456789abcdef";
      std::string encoded;
      for (unsigned char c : bytes) {
        encoded += hex[c >> 4];
        encoded += hex[c & 15];
      }
      request["ssid"] = encoded;
      request["security"] = security.value() == 0   ? "open"
                            : security.value() == 2 ? "sae"
                                                    : "wpa-psk";
    } else {
      request["ssid"] = row["ssid"];
      request["security"] = kind;
      request["path"] = row["path"];
    }
    apply(std::move(request));
  }
  secret.value("");
}
void NetworkPage::edit() {
  auto row = selected("profiles");
  if (row.is_null()) {
    report("Select a connection profile.");
    return;
  }
  auto uuid = row.at("uuid").get<std::string>();
  submit([uuid] { return Json{{"editor", network::profile(uuid)}}; });
}
void NetworkPage::editor(Json data) {
  Modal modal(modalOpen);
  dx::GpuWindow dialog(760, 470, "Connection profile");
  Fl_Input name(135, 20, 595, 32, "Name:");
  name.value(data.at("name").get<std::string>().c_str());
  Fl_Check_Button automatic(135, 62, 570, 28, "Connect automatically");
  automatic.value(data.value("autoconnect", true));
  Fl_Check_Button changeIp(135, 100, 570, 28,
                           "Change IP settings (leave off to preserve all settings)");
  Fl_Choice method4(135, 146, 230, 30, "IPv4 method:");
  method4.add("auto|manual|disabled|link-local|shared");
  Fl_Choice method6(500, 146, 230, 30, "IPv6 method:");
  method6.add("auto|manual|disabled|ignore|link-local|shared|dhcp");
  Fl_Input address4(135, 188, 230, 30, "Address/prefix:");
  Fl_Input address6(500, 188, 230, 30, "Address/prefix:");
  Fl_Input gateway4(135, 230, 230, 30, "Gateway:");
  Fl_Input gateway6(500, 230, 230, 30, "Gateway:");
  Fl_Input dns4(135, 272, 230, 30, "DNS servers:");
  Fl_Input dns6(500, 272, 230, 30, "DNS servers:");
  Fl_Check_Button override4(135, 312, 230, 28, "Override automatic DNS");
  Fl_Check_Button override6(500, 312, 230, 28, "Override automatic DNS");
  auto fill = [](const Json &ip, Fl_Choice &method, Fl_Input &address, Fl_Input &gateway,
                 Fl_Input &dns, Fl_Check_Button &overrideDns) {
    std::string value = ip.value("method", std::string("auto"));
    int index = 0;
    for (int i = 0; i < method.size() - 1; ++i)
      if (method.text(i) && value == method.text(i))
        index = i;
    method.value(index);
    address.value(ip.value("address", std::string()).c_str());
    gateway.value(ip.value("gateway", std::string()).c_str());
    dns.value(ip.value("dns", std::string()).c_str());
    overrideDns.value(ip.value("ignore_auto_dns", false));
  };
  fill(data.at("ipv4"), method4, address4, gateway4, dns4, override4);
  fill(data.at("ipv6"), method6, address6, gateway6, dns6, override6);
  Fl_Box note(30, 354, 700, 42,
              "IP changes apply on the next activation. Additional addresses and routes are "
              "preserved unless the address is changed.");
  note.align(FL_ALIGN_WRAP | FL_ALIGN_INSIDE | FL_ALIGN_LEFT);
  Fl_Button cancel(475, 418, 110, 32, "Cancel");
  Fl_Return_Button save(600, 418, 130, 32, "Save profile");
  bool accepted = false;
  struct Result {
    Fl_Window *window;
    bool *accepted;
  } result{&dialog, &accepted};
  save.callback(
      [](Fl_Widget *, void *p) {
        auto r = static_cast<Result *>(p);
        *r->accepted = true;
        r->window->hide();
      },
      &result);
  cancel.callback([](Fl_Widget *, void *p) { static_cast<Fl_Window *>(p)->hide(); }, &dialog);
  dialog.end();
  dialog.set_modal();
  dx::style(&dialog);
  dialog.show();
  while (dialog.shown())
    Fl::wait();
  if (!accepted)
    return;
  Json request = {{"action", "edit"},
                  {"uuid", data.at("uuid")},
                  {"name", std::string(name.value())},
                  {"autoconnect", bool(automatic.value())}};
  auto ip = [](Fl_Choice &method, Fl_Input &address, Fl_Input &gateway, Fl_Input &dns,
               Fl_Check_Button &overrideDns) {
    return Json{{"method", method.text() ? method.text() : "auto"},
                {"address", std::string(address.value())},
                {"gateway", std::string(gateway.value())},
                {"dns", std::string(dns.value())},
                {"ignore_auto_dns", bool(overrideDns.value())}};
  };
  if (changeIp.value()) {
    request["ipv4"] = ip(method4, address4, gateway4, dns4, override4);
    request["ipv6"] = ip(method6, address6, gateway6, dns6, override6);
  }
  apply(std::move(request));
}
void NetworkPage::import() {
  int type = fl_choice("Import which VPN profile type?", "Cancel", "OpenVPN", "WireGuard");
  if (type <= 0)
    return;
  Fl_Native_File_Chooser chooser;
  chooser.title("Import VPN profile");
  chooser.type(Fl_Native_File_Chooser::BROWSE_FILE);
  if (chooser.show())
    return;
  std::string path = chooser.filename();
  submit([path, type] {
    std::atomic_bool cancel{false};
    std::string output;
    int status = tasks::run(
        {"nmcli", "connection", "import", "type", type == 1 ? "openvpn" : "wireguard", "file",
         path},
        [&](const std::string &s) {
          if (output.size() < 16384)
            output += s;
        },
        cancel, 30);
    if (status)
      throw std::runtime_error(output.empty() ? "VPN import failed" : output);
    return network::snapshot();
  });
}
NetworkPage::NetworkPage(int X, int Y, int W, int H,
                         std::function<void(const std::string &)> status)
    : Fl_Group(X, Y, W, H, "Network"), report(std::move(status)) {
  networking = button(X + 16, Y + 16, 180, "Enable networking", [this] {
    if (state.value("enabled", false) &&
        fl_choice("Disconnect all network connections?", "Cancel", "Disable", nullptr) != 1)
      return;
    apply({{"action", "enabled"}, {"enabled", !state.value("enabled", false)}});
  });
  wireless = button(X + 210, Y + 16, 160, "Turn Wi-Fi on", [this] {
    apply({{"action", "wireless"}, {"enabled", !state.value("wireless", false)}});
  });
  button(X + 385, Y + 16, 100, "Refresh", [this] { refresh(); });
  devices = new Fl_Hold_Browser(X + 16, Y + 78, 270, 130, "Devices");
  devices->align(FL_ALIGN_TOP_LEFT);
  devices->format_char(0);
  networks = new Fl_Hold_Browser(X + 304, Y + 78, W - 320, 220, "Wi-Fi networks");
  networks->align(FL_ALIGN_TOP_LEFT);
  networks->format_char(0);
  button(X + 16, Y + 220, 125, "Disconnect", [this] {
    auto row = selected("devices");
    if (!row.is_null())
      apply({{"action", "disconnect"}, {"device", row["path"]}});
  });
  button(X + 150, Y + 220, 136, "Add wired…", [this] {
    auto row = selected("devices");
    if (!row.is_null())
      apply({{"action", "wired"}, {"device", row["path"]}});
  });
  button(X + 304, Y + 312, 100, "Scan", [this] {
    auto row = selected("devices");
    if (row.is_null()) {
      report("Select a Wi-Fi adapter.");
      return;
    }
    apply({{"action", "scan"}, {"device", row["path"]}});
  });
  button(X + 415, Y + 312, 130, "Connect…", [this] { connectWifi(); });
  button(X + 558, Y + 312, 150, "Hidden network…", [this] { connectWifi(true); });
  info = new Fl_Multiline_Output(X + 16, Y + 270, 270, H - 290);
  profiles = new Fl_Hold_Browser(X + 304, Y + 380, W - 320, 85, "Saved connections and VPNs");
  profiles->align(FL_ALIGN_TOP_LEFT);
  profiles->format_char(0);
  button(X + 304, Y + 480, 100, "Activate", [this] {
    auto row = selected("profiles");
    if (!row.is_null())
      apply({{"action", "activate"}, {"uuid", row["uuid"]}});
  });
  button(X + 415, Y + 480, 100, "Disconnect", [this] {
    auto row = selected("profiles");
    if (!row.is_null())
      apply({{"action", "deactivate"}, {"uuid", row["uuid"]}});
  });
  button(X + 526, Y + 480, 90, "Edit…", [this] { edit(); });
  button(X + 627, Y + 480, 100, "Remove", [this] {
    auto row = selected("profiles");
    if (!row.is_null() &&
        fl_choice("Remove the selected saved connection?", "Cancel", "Remove", nullptr) == 1)
      apply({{"action", "delete"}, {"uuid", row["uuid"]}});
  });
  button(X + 738, Y + 480, 145, "Import VPN…", [this] { import(); });
  end();
}
