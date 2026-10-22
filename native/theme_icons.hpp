// SPDX-License-Identifier: MIT
#pragma once
#include <glib.h>
#include <algorithm>
#include <cstdlib>
#include <filesystem>
#include <set>
#include <string>
#include <vector>
namespace theme_icons {
namespace fs = std::filesystem;
inline bool file(const fs::path &p) {
  std::error_code ec;
  return fs::is_regular_file(p, ec);
}
inline std::string resolve(const std::string &name, const std::string &extra,
                           int extent) {
  if (name.empty()) return {};
  if (fs::path(name).is_absolute()) return file(name) ? name : "";
  if (name.find('/') != std::string::npos || name == "..") return {};
  std::vector<fs::path> roots;
  if (!extra.empty()) roots.emplace_back(extra);
  roots.emplace_back(fs::path(g_get_user_data_dir()) / "icons");
  roots.emplace_back(fs::path(g_get_home_dir()) / ".icons");
  for (auto dirs = g_get_system_data_dirs(); *dirs; ++dirs)
    roots.emplace_back(fs::path(*dirs) / "icons");
  auto match = [&](const fs::path &dir) -> std::string {
    for (const auto &suffix : {"", ".svg", ".png", ".xpm"}) {
      auto p = dir / (name + suffix);
      if (file(p)) return p.string();
    }
    return {};
  };
  if (!extra.empty()) if (auto p = match(extra); !p.empty()) return p;
  std::vector<std::string> themes;
  // Honour the session's GTK icon theme without opening a display connection.
  auto settings = g_key_file_new();
  auto config = fs::path(g_get_user_config_dir()) / "gtk-3.0/settings.ini";
  if (g_key_file_load_from_file(settings, config.c_str(), G_KEY_FILE_NONE, nullptr)) {
    auto value = g_key_file_get_string(settings, "Settings", "gtk-icon-theme-name", nullptr);
    if (value) { themes.emplace_back(value); g_free(value); }
  }
  g_key_file_unref(settings);
  themes.insert(themes.end(), {"hicolor", "Adwaita", "breeze"});
  std::set<std::string> seen;
  for (size_t i = 0; i < themes.size() && i < 64; ++i) {
    const auto theme = themes[i];
    if (theme.find('/') != std::string::npos || theme == ".." || !seen.insert(theme).second) continue;
    std::string best; int score = 1000000;
    auto baseFor = [&](const fs::path &root) {
      return !extra.empty() && root == fs::path(extra) && file(root / "index.theme")
          ? root : root / theme;
    };
    // A user or /usr/local icon overlay normally has no index.theme of its own.
    // Use the theme metadata from the first available root for every overlay.
    auto index = g_key_file_new();
    bool loaded = false;
    for (const auto &root : roots)
      if (g_key_file_load_from_file(index, (baseFor(root) / "index.theme").c_str(), G_KEY_FILE_NONE, nullptr)) {
        loaded = true; break;
      }
    if (!loaded) { g_key_file_unref(index); continue; }
      auto inherits = g_key_file_get_string(index, "Icon Theme", "Inherits", nullptr);
      if (inherits) {
        auto parts = g_strsplit(inherits, ",", -1);
        std::vector<std::string> inherited;
        for (auto p = parts; *p; ++p) inherited.emplace_back(g_strstrip(*p));
        themes.insert(themes.begin() + i + 1, inherited.begin(), inherited.end());
        g_strfreev(parts); g_free(inherits);
      }
    for (const auto &root : roots) {
      auto base = baseFor(root);
      for (auto field : {"Directories", "ScaledDirectories"}) {
        auto value = g_key_file_get_string(index, "Icon Theme", field, nullptr);
        if (!value) continue;
        auto parts = g_strsplit(value, ",", -1);
        for (auto p = parts; *p; ++p) {
          auto dir = std::string(g_strstrip(*p));
          if (dir.empty() || fs::path(dir).is_absolute() || dir.find("..") != std::string::npos) continue;
          auto path = match(base / dir);
          if (path.empty()) continue;
          int size = g_key_file_get_integer(index, dir.c_str(), "Size", nullptr);
          int scale = g_key_file_get_integer(index, dir.c_str(), "Scale", nullptr);
          auto type = g_key_file_get_string(index, dir.c_str(), "Type", nullptr);
          int distance = type && std::string(type) == "Scalable" ? 0 : std::abs(size * std::max(1, scale) - extent);
          g_free(type);
          if (distance < score) { score = distance; best = path; }
        }
        g_strfreev(parts); g_free(value);
      }
    }
    g_key_file_unref(index);
    if (!best.empty()) return best;
  }
  for (const auto &root : roots) if (auto p = match(root); !p.empty()) return p;
  for (auto dirs = g_get_system_data_dirs(); *dirs; ++dirs)
    if (auto p = match(fs::path(*dirs) / "pixmaps"); !p.empty()) return p;
  return {};
}
} // namespace theme_icons
