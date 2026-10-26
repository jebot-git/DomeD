// SPDX-License-Identifier: MIT
#include "backup.h"
#include <chrono>
#include <csignal>
#include <iostream>
namespace {
volatile sig_atomic_t stopped = 0;
void stop(int) { stopped = 1; }
} // namespace
int main(int argc, char **argv) try {
  if (argc == 2 && std::string(argv[1]) == "--help") {
    std::cout << "dome-backup-job PROFILE-UUID\nRuns a saved rsync backup as the current user.\n";
    return 0;
  }
  if (argc != 2)
    throw std::runtime_error("Supply a saved backup profile UUID");
  backup::Json selected;
  for (auto &p : backup::profiles())
    if (p.at("id") == argv[1])
      selected = p;
  if (selected.is_null())
    throw std::runtime_error("Backup profile is missing or has unsafe permissions");
  if (selected.value("delete", false))
    throw std::runtime_error("Scheduled backups cannot delete destination-only files");
  std::signal(SIGTERM, stop);
  std::signal(SIGINT, stop);
  backup::Job job;
  job.start(selected, false);
  std::string previous;
  for (;;) {
    auto state = job.snapshot();
    if (state.output != previous) {
      std::cout << state.output.substr(state.output.rfind(previous, 0) == 0 ? previous.size() : 0)
                << std::flush;
      previous = state.output;
    }
    if (stopped)
      job.cancel();
    if (!state.running) {
      if (!state.error.empty())
        std::cerr << state.error << '\n';
      return state.status == 0 && !state.cancelled ? 0 : 1;
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
  }
} catch (const std::exception &e) {
  std::cerr << e.what() << '\n';
  return 1;
}
