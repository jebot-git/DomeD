// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once
#include <filesystem>
#include <fstream>
#include <nlohmann/json.hpp>
#include <csignal>
#include <cstdlib>
#include <string>
#include <vector>
#include <sys/stat.h>
#include <unistd.h>
namespace dome_pinentry {
// Askpass is started by sudo as the invoking user, often outside the compositor's
// process tree. Recover only that user's live session and exported bitmap fonts.
inline void session_environment(bool askpass) {
  namespace fs = std::filesystem;
  const char *runtime = getenv("XDG_RUNTIME_DIR");
  if (runtime && *runtime) try {
    const fs::path directory(runtime);
    std::vector<fs::path> markers;
    if (const char *socket = getenv("WAYLAND_DISPLAY")) {
      fs::path path(socket);
      markers.push_back(path.is_absolute() ? fs::path(path.string() + ".dome-session.json")
                                          : directory / (path.string() + ".dome-session.json"));
    }
    for (const auto &entry : fs::directory_iterator(directory))
      if (entry.path().filename().string().find(".dome-session.json") != std::string::npos)
        markers.push_back(entry.path());
    for (const auto &marker : markers) {
      struct stat st {};
      if (lstat(marker.c_str(), &st) || !S_ISREG(st.st_mode) || st.st_uid != getuid()) continue;
      nlohmann::json info;
      try { std::ifstream file(marker); file >> info; } catch (...) { continue; }
      const int pid = info.value("compositor_pid", 0);
      if (pid <= 0 || kill(pid, 0) != 0) continue;
      auto process = fs::path("/proc") / std::to_string(pid);
      if (stat(process.c_str(), &st) || st.st_uid != getuid()) continue;
      auto fonts = directory / ("dome-appearance-" + std::to_string(pid)) / "fonts.conf";
      if (!stat(fonts.c_str(), &st) && S_ISREG(st.st_mode) && st.st_uid == getuid())
        setenv("FONTCONFIG_FILE", fonts.c_str(), 1);
      setenv("XDG_CURRENT_DESKTOP", "Dome", 1);
      // Keep a valid inherited display, including an explicitly selected X11
      // test display. Recover the desktop socket only when it is unavailable.
      const char *socket = getenv("WAYLAND_DISPLAY");
      fs::path display = socket ? fs::path(socket) : fs::path();
      if (!display.empty() && !display.is_absolute()) display = directory / display;
      bool connected = !display.empty() && !stat(display.c_str(), &st) && S_ISSOCK(st.st_mode);
      const char *backend = getenv("FLTK_BACKEND");
      bool x11 = backend && std::string(backend) == "x11";
      if (askpass && !connected && !x11 && info.value("realm", "") != "desktop") continue;
      if (askpass && !connected && !x11 && info.value("realm", "") == "desktop") {
        auto name = marker.filename().string();
        name.resize(name.size() - std::string(".dome-session.json").size());
        auto candidate = directory / name;
        if (!stat(candidate.c_str(), &st) && S_ISSOCK(st.st_mode)) {
          setenv("WAYLAND_DISPLAY", name.c_str(), 1);
          unsetenv("DISPLAY");
        }
      }
      break;
    }
  } catch (...) {}
  // This is DomeD's explicitly selected sudo frontend even when the caller
  // inherited another desktop's environment. Its palette comes from DomeD.
  if (askpass) setenv("XDG_CURRENT_DESKTOP", "Dome", 1);
}
} // namespace dome_pinentry
