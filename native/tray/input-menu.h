// SPDX-License-Identifier: MIT
#pragma once
#include <functional>
#include <memory>
#include <string>
#include <gio/gio.h>
// The keyboard selector consumes this menu without registering another SNI icon.
class InputMenu {
  struct Impl;
  std::unique_ptr<Impl> impl;
public:
  InputMenu(GDBusConnection *, std::function<void(const std::string &)>);
  ~InputMenu();
};
