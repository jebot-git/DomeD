// SPDX-License-Identifier: MIT
#pragma once
#include <memory>
#include <nlohmann/json.hpp>
#include <string>
namespace bluetooth {
using Json = nlohmann::json;
// Owns a system-bus connection and GLib context on a worker thread. No radio
// state changes occur until requested; discovery sessions are bounded/owned.
class Client {
  struct Impl;
  std::unique_ptr<Impl> impl;

public:
  Client();
  ~Client();
  Json snapshot() const;
  void refresh();
  void action(const std::string &path, const std::string &operation);
  void respond(unsigned id, bool accept, const std::string &value = "");
  void stopDiscovery();
  void cancelPairing();
};
} // namespace bluetooth
