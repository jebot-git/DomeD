// SPDX-License-Identifier: MIT
#pragma once
#include <functional>
#include <memory>
#include <string>
class Notifier {
  struct Impl;
  std::unique_ptr<Impl> impl;

public:
  explicit Notifier(std::function<void(const std::string &)> show);
  ~Notifier();
  void setSelectionEnabled(bool enabled);
  static bool showExisting(const std::string &page);
};
