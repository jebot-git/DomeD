// SPDX-License-Identifier: MIT
#pragma once
#include "network.h"
#include <FL/Fl_Button.H>
#include <FL/Fl_Group.H>
#include <FL/Fl_Hold_Browser.H>
#include <FL/Fl_Multiline_Output.H>
#include <deque>
#include <functional>
#include <future>
class NetworkPage : public Fl_Group {
  Fl_Hold_Browser *devices, *networks, *profiles;
  Fl_Multiline_Output *info;
  Fl_Button *wireless, *networking;
  network::Json state = network::Json::object();
  std::future<network::Json> pending;
  double nextRefresh = 0;
  bool modalOpen = false;
  std::deque<std::function<void()>> callbacks;
  std::function<void(const std::string &)> report;
  void on(Fl_Widget *, std::function<void()>);
  Fl_Button *button(int, int, int, const char *, std::function<void()>);
  network::Json selected(const char *) const;
  void submit(std::function<network::Json()>);
  void apply(network::Json);
  void update(network::Json);
  void connectWifi(bool hidden = false);
  void edit();
  void import();
  void editor(network::Json);

public:
  NetworkPage(int, int, int, int, std::function<void(const std::string &)>);
  void refresh();
  void tick();
  void clearSecrets() {}
};
