// SPDX-License-Identifier: MIT
#pragma once
#include <gio/gio.h>
#include <memory>
#include <nlohmann/json.hpp>
#include <string>
#include <vector>
namespace services {
using Json = nlohmann::json;
struct Unref {
  void operator()(GVariant *v) const {
    if (v)
      g_variant_unref(v);
  }
};
using Variant = std::unique_ptr<GVariant, Unref>;
Variant call(const std::string &service, const std::string &path,
             const std::string &interface, const std::string &method,
             GVariant *arguments = nullptr);
Json devices();
Json device(const std::string &id);
void deviceAction(const std::string &id, const std::string &action,
                  const std::string &value);
Json methods();
void inputAction(const std::string &action, const std::string &value);
Json inputGroup(const std::string &group);
void saveGroup(const std::string &group, const std::string &layout,
               const Json &methods);
Json inputConfig(const std::string &uri);
void saveInputConfig(const std::string &uri, const Json &values);
Json wallets();
void walletLifecycle(const std::string &action, const std::string &name);
int openWallet(const std::string &name);
Json folders(int handle);
Json entries(int handle, const std::string &folder);
Json readData(int handle, const std::string &folder, const std::string &key);
void writeData(int handle, const std::string &folder, const std::string &key,
               const Json &data);
std::string readPassword(int handle, const std::string &folder,
                         const std::string &key);
void walletAction(const std::string &action, int handle,
                  const std::string &folder, const std::string &key,
                  const std::string &value);
Json trayItems();
void trayPlacement(const Json &item, bool managed);
Json trayMenu(const Json &item, int parent);
void trayEvent(const Json &item, int id);
std::string frontend(const std::string &identity);
} // namespace services
