// SPDX-License-Identifier: MIT
#pragma once
#include <NetworkManager.h>
#include <nlohmann/json.hpp>
namespace network {
using Json = nlohmann::json;
Json snapshot();
Json profile(const std::string &uuid);
void action(const Json &request);
// Construct and validate settings separately from activating them.
NMConnection *wifiConnection(const Json &request);
void editProfile(NMConnection *connection, const Json &settings);
} // namespace network
