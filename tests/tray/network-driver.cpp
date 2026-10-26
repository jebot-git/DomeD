// SPDX-License-Identifier: MIT
#include "network.h"
#include <iostream>
int main() {
  std::string line;
  while (std::getline(std::cin, line)) {
    try {
      auto request = network::Json::parse(line);
      network::Json result;
      auto op = request.at("op").get<std::string>();
      if (op == "snapshot")
        result = network::snapshot();
      else if (op == "profile")
        result = network::profile(request.at("uuid"));
      else if (op == "action")
        network::action(request.at("request"));
      else
        throw std::runtime_error("Unknown test operation");
      std::cout << network::Json{{"ok", true}, {"value", result}} << std::endl;
    } catch (const std::exception &e) {
      std::cout << network::Json{{"ok", false}, {"error", e.what()}} << std::endl;
    }
  }
}
