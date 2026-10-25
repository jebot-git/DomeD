// SPDX-License-Identifier: MIT
#include <algorithm>
#include <filesystem>
#include <gio/gdesktopappinfo.h>
#include <glib/gstdio.h>
#include <nlohmann/json.hpp>
#include <set>
#include <string>
using J = nlohmann::json;
struct FileType {
  const char *mime, *label;
  bool archive = false;
};
static const FileType types[] = {
    {"inode/directory", "Folders"},
    {"text/plain", "Text (.txt)"},
    {"text/markdown", "Markdown (.md)"},
    {"application/pdf", "PDF documents"},
    {"text/html", "HTML documents"},
    {"application/json", "JSON files"},
    {"text/csv", "CSV tables"},
    {"application/vnd.openxmlformats-officedocument.wordprocessingml.document",
     "Word documents (.docx)"},
    {"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
     "Excel spreadsheets (.xlsx)"},
    {"application/"
     "vnd.openxmlformats-officedocument.presentationml.presentation",
     "PowerPoint slides (.pptx)"},
    {"application/vnd.oasis.opendocument.text", "OpenDocument text (.odt)"},
    {"application/vnd.oasis.opendocument.spreadsheet",
     "OpenDocument spreadsheets (.ods)"},
    {"application/vnd.oasis.opendocument.presentation",
     "OpenDocument slides (.odp)"},
    {"application/msword", "Word documents (.doc)"},
    {"application/vnd.ms-excel", "Excel spreadsheets (.xls)"},
    {"application/vnd.ms-powerpoint", "PowerPoint slides (.ppt)"},
    {"image/png", "PNG images"},
    {"image/jpeg", "JPEG images"},
    {"image/gif", "GIF images"},
    {"image/webp", "WebP images"},
    {"image/svg+xml", "SVG images"},
    {"audio/mpeg", "MP3 audio"},
    {"audio/flac", "FLAC audio"},
    {"audio/ogg", "Ogg audio"},
    {"video/mp4", "MP4 video"},
    {"video/webm", "WebM video"},
    {"video/x-matroska", "Matroska video"},
    {"application/zip", "ZIP archives", true},
    {"application/x-7z-compressed", "7-Zip archives", true},
    {"application/x-tar", "Tar archives", true},
    {"application/x-compressed-tar", "Tar + gzip archives", true},
    {"application/x-bzip-compressed-tar", "Tar + bzip2 archives", true},
    {"application/x-xz-compressed-tar", "Tar + xz archives", true},
    {"application/x-lzma-compressed-tar", "Tar + LZMA archives", true},
    {"application/x-lzip-compressed-tar", "Tar + lzip archives", true},
    {"application/x-lz4-compressed-tar", "Tar + LZ4 archives", true},
    {"application/x-zstd-compressed-tar", "Tar + Zstandard archives", true},
    {"application/x-lzop-compressed-tar", "Tar + lzop archives", true},
    {"application/x-cpio", "CPIO archives", true},
    {"application/x-cpio-compressed", "Compressed CPIO archives", true},
    {"application/x-archive", "ar archives", true},
    {"application/x-cd-image", "ISO disc images", true},
    {"application/x-xar", "XAR archives", true},
    {"application/warc", "WARC archives", true},
    {"application/x-mtree", "mtree metadata manifests", true},
    {"application/vnd.rar", "RAR archives", true},
    {"application/vnd.ms-cab-compressed", "CAB archives", true},
    {"application/x-lha", "LHA / LZH archives", true},
    {"application/x-rpm", "RPM packages", true},
    {"application/vnd.debian.binary-package", "Debian packages", true},
    {"application/gzip", "gzip streams", true},
    {"application/x-bzip2", "bzip2 streams", true},
    {"application/x-xz", "xz streams", true},
    {"application/x-lzma", "LZMA streams", true},
    {"application/x-lzip", "lzip streams", true},
    {"application/x-lz4", "LZ4 streams", true},
    {"application/zstd", "Zstandard streams", true},
    {"application/x-compress", "compress streams", true},
    {"application/x-lzop", "lzop streams", true},
    {"application/x-bittorrent", "BitTorrent files (.torrent)"},
    {"x-scheme-handler/magnet", "Magnet links"},
    {"x-scheme-handler/https", "Web links (https)"},
    {"x-scheme-handler/http", "Web links (http)"},
    {"x-scheme-handler/mailto", "Email links"},
};
static bool archiveType(const char *mime) {
  for (auto type : types)
    if (type.archive && g_str_equal(type.mime, mime)) return true;
  return false;
}
static std::string configPath() {
  return std::string(g_get_user_config_dir()) + "/dome-mimeapps.list";
}
static GKeyFile *preferences(std::string &message) {
  auto file = g_key_file_new();
  GError *e = nullptr;
  if (!g_key_file_load_from_file(file, configPath().c_str(),
                                 GKeyFileFlags(G_KEY_FILE_KEEP_COMMENTS |
                                               G_KEY_FILE_KEEP_TRANSLATIONS),
                                 &e)) {
    if (!g_error_matches(e, G_FILE_ERROR, G_FILE_ERROR_NOENT))
      message = e->message;
    g_clear_error(&e);
  }
  return file;
}
static std::string executable(GAppInfo *app) {
  auto cmd = g_app_info_get_commandline(app);
  gchar **args = nullptr;
  gint count = 0;
  if (!cmd || !g_shell_parse_argv(cmd, &count, &args, nullptr) || count < 1) {
    g_strfreev(args);
    return {};
  }
  std::string path = args[0];
  g_strfreev(args);
  for (size_t pos = 0; (pos = path.find("%%", pos)) != std::string::npos; ++pos)
    path.erase(pos, 1);
  return path;
}
static bool available(GAppInfo *app) {
  if (!g_app_info_get_id(app) ||
      g_desktop_app_info_get_is_hidden(G_DESKTOP_APP_INFO(app)))
    return false;
  auto path = g_find_program_in_path(executable(app).c_str());
  bool ok = path;
  g_free(path);
  return ok;
}
static bool supports(GAppInfo *app, const char *mime) {
  auto list = g_app_info_get_supported_types(app);
  if (list)
    for (auto p = list; *p; ++p)
      if (g_content_type_is_a(mime, *p) || g_str_equal(mime, *p))
        return true;
  return false;
}
static J describe(GAppInfo *app) {
  return {{"id", g_app_info_get_id(app)},
          {"name", g_app_info_get_display_name(app)}};
}
static GAppInfo *scoped(GKeyFile *file, const char *mime) {
  auto ids = g_key_file_get_string_list(file, "Default Applications", mime,
                                        nullptr, nullptr);
  GAppInfo *current = nullptr;
  if (ids)
    for (auto id = ids; *id && !current; ++id) {
      auto app = g_desktop_app_info_new(*id);
      if (app) {
        if (available(G_APP_INFO(app)))
          current = G_APP_INFO(app);
        else
          g_object_unref(app);
      }
    }
  g_strfreev(ids);
  return current;
}
static std::string save(GKeyFile *file) {
  if (g_mkdir_with_parents(g_get_user_config_dir(), 0700))
    return "Cannot create user configuration directory";
  gsize size = 0;
  auto data = g_key_file_to_data(file, &size, nullptr);
  GError *e = nullptr;
  bool ok = g_file_set_contents_full(
      configPath().c_str(), data, size,
      GFileSetContentsFlags(G_FILE_SET_CONTENTS_CONSISTENT |
                            G_FILE_SET_CONTENTS_DURABLE),
      0600, &e);
  g_free(data);
  std::string message = ok ? "" : e->message;
  g_clear_error(&e);
  return message;
}
static std::string seed(GKeyFile *file, const char *mime) {
  if (g_key_file_has_key(file, "Default Applications", mime, nullptr))
    return {};
  auto apps = g_app_info_get_all_for_type(mime);
  GAppInfo *preferred = nullptr;
  int rank = 100;
  for (auto node = apps; node; node = node->next) {
    auto app = G_APP_INFO(node->data);
    if (!available(app) || !supports(app, mime))
      continue;
    auto base = std::filesystem::path(executable(app)).filename().string();
    int candidate = 100;
    if (g_str_equal(mime, "application/pdf")) {
      if (base == "miro-pdf")
        candidate =
            g_str_equal(g_app_info_get_id(app), "miro-pdf.desktop") ? 0 : 1;
    } else if (g_str_equal(mime, "application/x-bittorrent") ||
               g_str_equal(mime, "x-scheme-handler/magnet")) {
      // The canonical desktop entry may launch through dome-link. Its ID is
      // stable across routing wrappers; TryExec checks the actual client.
      if (g_str_equal(g_app_info_get_id(app), "org.domed.Torrent.desktop"))
        candidate = 0;
      else if (base == "dome-torrent")
        candidate = 1;
    } else if (archiveType(mime)) {
      if (g_str_equal(g_app_info_get_id(app), "org.fsvxr.fsvr.desktop")) candidate = 0;
      else if (base == "fsvr") candidate = 1;
    } else if (base == "fsvr")
      candidate = 0;
    else if (base == "fsx-vr")
      candidate = 1;
    else if (base == "fsv-xr")
      candidate = 2;
    if (candidate < rank) {
      preferred = app;
      rank = candidate;
    }
  }
  std::string error;
  if (preferred) {
    auto id = g_app_info_get_id(preferred);
    g_key_file_set_string_list(file, "Default Applications", mime, &id, 1);
    if (g_str_equal(mime, "inode/directory") &&
        !g_key_file_has_key(file, "Default Applications",
                            "x-scheme-handler/trash", nullptr) &&
        supports(preferred, "x-scheme-handler/trash"))
      g_key_file_set_string_list(file, "Default Applications",
                                 "x-scheme-handler/trash", &id, 1);
    error = save(file);
  }
  g_list_free_full(apps, g_object_unref);
  return error;
}
extern "C" const char *dome_default_apps_list() {
  thread_local std::string result;
  std::string error;
  auto file = preferences(error);
  if (error.empty())
    error = seed(file, "inode/directory");
  if (error.empty())
    error = seed(file, "application/pdf");
  if (error.empty())
    error = seed(file, "application/x-bittorrent");
  if (error.empty())
    error = seed(file, "x-scheme-handler/magnet");
  for (auto type : types)
    if (error.empty() && type.archive) error = seed(file, type.mime);
  J rows = J::array();
  for (auto type : types) {
    auto apps = g_app_info_get_all_for_type(type.mime);
    std::vector<J> choices;
    for (auto node = apps; node; node = node->next) {
      auto app = G_APP_INFO(node->data);
      if (available(app))
        choices.push_back(describe(app));
    }
    auto current = scoped(file, type.mime);
    if (!current && !g_str_equal(type.mime, "application/pdf"))
      current = g_app_info_get_default_for_type(type.mime, false);
    J selected = nullptr;
    if (current) {
      selected = describe(current);
      if (available(current))
        choices.push_back(selected);
      g_object_unref(current);
    }
    std::sort(choices.begin(), choices.end(), [](auto &a, auto &b) {
      return g_utf8_collate(a["name"].template get<std::string>().c_str(),
                            b["name"].template get<std::string>().c_str()) < 0;
    });
    J unique = J::array();
    std::set<std::string> seen;
    for (auto &choice : choices)
      if (seen.insert(choice["id"].get<std::string>()).second)
        unique.push_back(choice);
    rows.push_back({{"mime", type.mime},
                    {"label", type.label},
                    {"current", selected},
                    {"apps", unique}});
    g_list_free_full(apps, g_object_unref);
  }
  g_key_file_unref(file);
  result = J{{"rows", rows}, {"error", error}}.dump();
  return result.c_str();
}
extern "C" const char *dome_default_app_set(const char *mime, const char *id) {
  thread_local std::string error;
  error.clear();
  bool known = false;
  for (auto t : types)
    if (g_str_equal(mime, t.mime))
      known = true;
  if (!known) {
    error = "Unknown file type";
    return error.c_str();
  }
  auto app = g_desktop_app_info_new(id);
  if (!app || !available(G_APP_INFO(app))) {
    if (app)
      g_object_unref(app);
    error = "The selected application is no longer installed";
    return error.c_str();
  }
  if (!supports(G_APP_INFO(app), mime)) {
    g_object_unref(app);
    error = "The application does not declare support for this file type";
    return error.c_str();
  }
  auto file = preferences(error);
  if (error.empty()) {
    g_key_file_set_string_list(file, "Default Applications", mime, &id, 1);
    error = save(file);
  }
  g_key_file_unref(file);
  g_object_unref(app);
  return error.c_str();
}
