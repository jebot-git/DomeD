// SPDX-License-Identifier: MIT
#pragma once
#include <atomic>
#include <functional>
#include <string>
#include <vector>
namespace tasks {
int run(const std::vector<std::string> &arguments,
        const std::function<void(const std::string &)> &output, const std::atomic_bool &cancel,
        int timeoutSeconds = 0);
}
