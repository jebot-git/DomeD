// SPDX-License-Identifier: MIT
#pragma once
#include "backup.h"
#include <FL/Fl_Box.H>
#include <FL/Fl_Check_Button.H>
#include <FL/Fl_Choice.H>
#include <FL/Fl_Group.H>
#include <FL/Fl_Input.H>
#include <FL/Fl_Multiline_Input.H>
#include <FL/Fl_Multiline_Output.H>
#include <deque>
#include <functional>
#include <future>
class BackupPage : public Fl_Group {
  Fl_Choice *saved, *period;
  Fl_Input *name, *source, *destination;
  Fl_Check_Button *remove, *checksum, *compress;
  Fl_Multiline_Input *excludes;
  Fl_Multiline_Output *log;
  Fl_Box *state;
  backup::Json loadedIdentity;
  std::string loadedDestination;
  backup::Json rows = backup::Json::array();
  std::string uuid, previewed, jobSettings, lastOutput;
  backup::Job job;
  bool wasRunning = false;
  std::future<void> pending;
  std::function<void(const std::string &)> report;
  std::deque<std::function<void()>> callbacks;
  void on(Fl_Widget *, std::function<void()>);
  void button(int, int, int, const char *, std::function<void()>);
  backup::Json form() const;
  void load(const backup::Json &);
  void start(bool);
  void restore();

public:
  BackupPage(int, int, int, int, std::function<void(const std::string &)>);
  void refresh();
  void tick();
};
