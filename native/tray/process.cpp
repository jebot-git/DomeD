// SPDX-License-Identifier: MIT
#include "process.h"
#include <chrono>
#include <csignal>
#include <gio/gio.h>
#include <stdexcept>
#include <thread>
#include <unistd.h>
namespace tasks {
int run(const std::vector<std::string> &arguments,
        const std::function<void(const std::string &)> &output, const std::atomic_bool &cancel,
        int timeoutSeconds) {
  if (arguments.empty())
    throw std::runtime_error("Missing program");
  std::vector<const char *> argv;
  for (const auto &a : arguments)
    argv.push_back(a.c_str());
  argv.push_back(nullptr);
  auto launcher = g_subprocess_launcher_new(GSubprocessFlags(G_SUBPROCESS_FLAGS_STDIN_PIPE |
                                                             G_SUBPROCESS_FLAGS_STDOUT_PIPE |
                                                             G_SUBPROCESS_FLAGS_STDERR_MERGE));
  g_subprocess_launcher_setenv(launcher, "LC_ALL", "C.UTF-8", TRUE);
  g_subprocess_launcher_set_child_setup(
      launcher, [](gpointer) { setpgid(0, 0); }, nullptr, nullptr);
  GError *error = nullptr;
  auto child = g_subprocess_launcher_spawnv(launcher, argv.data(), &error);
  g_object_unref(launcher);
  if (!child) {
    std::string message = error ? error->message : "Cannot start process";
    g_clear_error(&error);
    throw std::runtime_error(message);
  }
  auto id = g_subprocess_get_identifier(child);
  pid_t pid = id ? std::stoi(id) : 0;
  g_output_stream_close(g_subprocess_get_stdin_pipe(child), nullptr, nullptr);
  std::atomic_bool done{false};
  bool timedOut = false;
  auto began = std::chrono::steady_clock::now();
  std::thread monitor([&] {
    bool sent = false;
    auto stopped = began;
    while (!done) {
      auto now = std::chrono::steady_clock::now();
      bool expired = timeoutSeconds > 0 && now - began > std::chrono::seconds(timeoutSeconds);
      if (cancel || expired) {
        if (expired)
          timedOut = true;
        if (!sent) {
          if (pid > 1)
            kill(-pid, SIGTERM);
          sent = true;
          stopped = now;
        } else if (now - stopped > std::chrono::seconds(2) && pid > 1)
          kill(-pid, SIGKILL);
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(40));
    }
  });
  std::exception_ptr failure;
  try {
    char buffer[4096];
    auto stream = g_subprocess_get_stdout_pipe(child);
    gssize count;
    while ((count = g_input_stream_read(stream, buffer, sizeof buffer, nullptr, &error)) > 0)
      output(std::string(buffer, count));
    if (count < 0) {
      std::string message = error ? error->message : "Cannot read job output";
      g_clear_error(&error);
      throw std::runtime_error(message);
    }
  } catch (...) {
    failure = std::current_exception();
    if (pid > 1)
      kill(-pid, SIGKILL);
  }
  g_subprocess_wait(child, nullptr, nullptr);
  done = true;
  monitor.join();
  int status = g_subprocess_get_if_exited(child) ? g_subprocess_get_exit_status(child)
                                                 : 128 + g_subprocess_get_term_sig(child);
  g_object_unref(child);
  if (failure)
    std::rethrow_exception(failure);
  if (timedOut)
    throw std::runtime_error("The operation timed out");
  return status;
}
} // namespace tasks
