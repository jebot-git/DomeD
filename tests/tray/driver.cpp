// SPDX-License-Identifier: MIT
#include "services.h"
#include <iostream>
using namespace services;
int main() {
  std::string line;
  while (std::getline(std::cin, line)) {
    try {
      auto r = Json::parse(line);
      std::string op = r.at("op");
      Json out;
      if (op == "devices")
        out = devices();
      else if (op == "device")
        out = device(r.at("id"));
      else if (op == "device-action")
        deviceAction(r.value("id", std::string()), r.at("action"),
                     r.value("value", std::string()));
      else if (op == "methods")
        out = methods();
      else if (op == "input-action")
        inputAction(r.at("action"), r.value("value", std::string()));
      else if (op == "group")
        out = inputGroup(r.at("group"));
      else if (op == "save-group")
        saveGroup(r.at("group"), r.at("layout"), r.at("methods"));
      else if (op == "config")
        out = inputConfig(r.at("uri"));
      else if (op == "save-config")
        saveInputConfig(r.at("uri"), r.at("values"));
      else if (op == "wallets")
        out = wallets();
      else if (op == "open")
        out = openWallet(r.at("name"));
      else if (op == "folders")
        out = folders(r.at("handle"));
      else if (op == "entries")
        out = entries(r.at("handle"), r.at("folder"));
      else if (op == "password")
        out = readPassword(r.at("handle"), r.at("folder"), r.at("key"));
      else if (op == "read-data")
        out = readData(r.at("handle"), r.at("folder"), r.at("key"));
      else if (op == "write-data")
        writeData(r.at("handle"), r.at("folder"), r.at("key"), r.at("data"));
      else if (op == "wallet-action")
        walletAction(
            r.at("action"), r.at("handle"), r.value("folder", std::string()),
            r.value("key", std::string()), r.value("value", std::string()));
      else if (op == "tray")
        out = trayItems();
      else if (op == "placement")
        trayPlacement(r.at("item"), r.at("managed"));
      else if (op == "menu")
        out = trayMenu(r.at("item"), r.at("parent"));
      else if (op == "event")
        trayEvent(r.at("item"), r.at("id"));
      else
        throw std::runtime_error("Unknown test operation");
      std::cout << Json{{"ok", true}, {"value", out}}.dump() << std::endl;
    } catch (const std::exception &e) {
      std::cout << Json{{"ok", false}, {"error", e.what()}}.dump() << std::endl;
    }
  }
}
