// SPDX-License-Identifier: MIT
#include "bluetooth.h"
#include <iostream>
int main() {
  bluetooth::Client client;
  std::string line;
  while (std::getline(std::cin, line)) {
    try {
      auto r = bluetooth::Json::parse(line);
      std::string op = r.at("op");
      if (op == "action")
        client.action(r.at("path"), r.at("action"));
      else if (op == "answer")
        client.respond(r.at("id"), r.at("accept"),
                       r.value("value", std::string()));
      else if (op == "cancel")
        client.cancelPairing();
      else if (op == "stop")
        client.stopDiscovery();
      else if (op == "refresh")
        client.refresh();
      else if (op != "state")
        throw std::runtime_error("Unknown operation");
      std::cout << client.snapshot().dump() << std::endl;
    } catch (const std::exception &e) {
      std::cout << bluetooth::Json{{"error", e.what()}}.dump() << std::endl;
    }
  }
}
