// SPDX-License-Identifier: MIT
#pragma once
#include <atomic>
#include <mutex>
#include <nlohmann/json.hpp>
#include <thread>
#include <vector>
namespace backup {
using Json = nlohmann::json;
void validate(const Json &);
void identify(Json &, bool replace = false);
std::vector<std::string> arguments(const Json &, bool preview, const std::string &stamp);
Json profiles();
void save(const Json &);
void schedule(const Json &, const std::string &period);
void unschedule(const std::string &uuid);
struct State {
  bool running = false, preview = false, cancelled = false;
  int status = -1;
  std::string output, error;
};
class Job {
  mutable std::mutex mutex;
  State state;
  std::atomic_bool stopping{false};
  std::thread worker;

public:
  ~Job();
  State snapshot() const;
  void start(const Json &, bool preview);
  void cancel();
};
} // namespace backup
