// SPDX-License-Identifier: MIT
#include "backup.h"
#include "process.h"
#include <algorithm>
#include <chrono>
#include <fcntl.h>
#include <filesystem>
#include <fstream>
#include <gio/gio.h>
#include <regex>
#include <sstream>
#include <stdexcept>
#include <sys/file.h>
#include <sys/stat.h>
#include <sys/vfs.h>
#include <unistd.h>
namespace backup {
namespace fs = std::filesystem;
namespace {
[[noreturn]] void fail(const std::string &s) { throw std::runtime_error(s); }
fs::path config() { return fs::path(g_get_user_config_dir()) / "dome/backups"; }
void id(const std::string &s) {
  if (!g_uuid_string_is_valid(s.c_str()))
    fail("Invalid backup profile identifier");
}
struct Location {
  std::string host, path;
};
Location location(const std::string &s) {
  if (s.empty() || s.size() > 8192 || s.find_first_of("\r\n\0", 0, 3) != std::string::npos)
    fail("Enter a source and destination directory");
  if (s[0] == '/')
    return {{}, s};
  static const std::regex remote(
      R"(^((?:[A-Za-z0-9_.-]+@)?(?:[A-Za-z0-9][A-Za-z0-9_.-]*|\[[0-9a-fA-F:]+\])):(/.*)$)");
  std::smatch match;
  if (!std::regex_match(s, match, remote))
    fail("Use an absolute local directory or user@host:/absolute/directory");
  return {match[1], match[2]};
}
bool within(const fs::path &a, const fs::path &b) {
  auto ai = a.begin(), bi = b.begin();
  for (; bi != b.end(); ++bi, ++ai)
    if (ai == a.end() || *ai != *bi)
      return false;
  return true;
}
std::string trimSlash(std::string p) {
  while (p.size() > 1 && p.back() == '/')
    p.pop_back();
  return p;
}
void write(const fs::path &path, const std::string &contents) {
  fs::create_directories(path.parent_path());
  chmod(path.parent_path().c_str(), 0700);
  auto file = g_file_new_for_path(path.c_str());
  GError *error = nullptr;
  bool okay = g_file_replace_contents(file, contents.data(), contents.size(), nullptr, FALSE,
                                      G_FILE_CREATE_PRIVATE, nullptr, nullptr, &error);
  g_object_unref(file);
  if (!okay) {
    std::string message = error ? error->message : "Cannot save backup profile";
    g_clear_error(&error);
    fail(message);
  }
}
void userctl(const std::vector<std::string> &args) {
  std::atomic_bool cancel{false};
  std::string output;
  std::vector<std::string> command{"systemctl", "--user"};
  command.insert(command.end(), args.begin(), args.end());
  int status = tasks::run(
      command,
      [&](const std::string &s) {
        if (output.size() < 16384)
          output += s;
      },
      cancel, 15);
  if (status)
    fail(output.empty() ? "Cannot configure the backup timer" : output);
}
} // namespace
void identify(Json &p, bool replace) {
  auto dest = location(p.at("destination").get<std::string>());
  if (!dest.host.empty()) {
    p.erase("destination_identity");
    return;
  }
  if (!replace && p.contains("destination_identity"))
    return;
  if (!fs::is_directory(dest.path))
    fail("Choose an existing destination directory");
  struct statfs info;
  if (statfs(dest.path.c_str(), &info))
    fail("Cannot inspect the destination filesystem");
  p["destination_identity"] = {{"path", fs::canonical(dest.path).string()},
                               {"filesystem", {info.f_fsid.__val[0], info.f_fsid.__val[1]}}};
}
void validate(const Json &p) {
  auto source = location(p.at("source").get<std::string>()),
       destination = location(p.at("destination").get<std::string>());
  if (!source.host.empty() && !destination.host.empty())
    fail("One side of the backup must be local");
  if (destination.host.empty() && !fs::is_directory(destination.path))
    fail("Choose an existing backup destination directory");
  if (source.host.empty() && !fs::is_directory(source.path))
    fail("The source must be an existing directory");
  if (source.host == destination.host) {
    auto a = source.host.empty() ? fs::weakly_canonical(source.path)
                                 : fs::path(source.path).lexically_normal();
    auto b = destination.host.empty() ? fs::weakly_canonical(destination.path)
                                      : fs::path(destination.path).lexically_normal();
    if (within(a, b) || within(b, a))
      fail("Source and destination must be separate, non-overlapping directories");
  }
  if (!destination.host.empty() && destination.path == "/")
    fail("Choose a backup directory rather than the remote filesystem root");
  if (destination.host.empty()) {
    if (fs::exists(destination.path) && !fs::is_directory(destination.path))
      fail("The backup destination is not a directory");
    if (fs::weakly_canonical(destination.path) == fs::path("/"))
      fail("Choose a backup directory rather than the filesystem root");
  }
  if (p.contains("destination_identity") && destination.host.empty()) {
    auto current = p;
    identify(current, true);
    if (current.at("destination_identity") != p.at("destination_identity"))
      fail("The backup filesystem or mount changed. Re-select the intended destination and save "
           "the profile.");
  }
  auto excludes = p.value("excludes", std::string());
  if (excludes.size() > 16384 || excludes.find('\0') != std::string::npos)
    fail("Invalid exclude patterns");
}
std::vector<std::string> arguments(const Json &p, bool preview, const std::string &stamp) {
  validate(p);
  if (stamp.empty() || stamp.find_first_not_of("0123456789-") != std::string::npos)
    fail("Invalid backup run identifier");
  auto source = trimSlash(p.at("source").get<std::string>()),
       destination = trimSlash(p.at("destination").get<std::string>());
  auto target = location(destination);
  std::vector<std::string> result{"rsync",
                                  "--archive",
                                  "--no-devices",
                                  "--no-specials",
                                  "--human-readable",
                                  "--itemize-changes",
                                  "--info=progress2",
                                  "--secluded-args",
                                  "--backup",
                                  "--exclude=/.de-xr-history/",
                                  "--backup-dir=" + trimSlash(target.path) + "/.de-xr-history/" +
                                      stamp};
  if (preview)
    result.push_back("--dry-run");
  if (p.value("delete", false))
    result.push_back("--delete-delay");
  if (p.value("checksum", false))
    result.push_back("--checksum");
  if (p.value("compress", false))
    result.push_back("--compress");
  if (!location(source).host.empty() || !target.host.empty())
    result.push_back("--rsh=ssh -oBatchMode=yes -oStrictHostKeyChecking=yes");
  std::istringstream lines(p.value("excludes", std::string()));
  std::string line;
  while (std::getline(lines, line)) {
    if (!line.empty() && line.back() == '\r')
      line.pop_back();
    if (!line.empty())
      result.push_back("--exclude=" + line);
  }
  result.push_back("--");
  result.push_back(source + "/");
  result.push_back(destination + "/");
  return result;
}
Json profiles() {
  Json result = Json::array();
  if (!fs::is_directory(config()))
    return result;
  for (const auto &e : fs::directory_iterator(config()))
    if (e.is_regular_file() && e.path().extension() == ".json") {
      struct stat info;
      if (lstat(e.path().c_str(), &info) || !S_ISREG(info.st_mode) || info.st_uid != getuid() ||
          (info.st_mode & 0077) || info.st_size > 65536)
        continue;
      try {
        std::ifstream stream(e.path());
        auto p = Json::parse(stream);
        id(p.at("id"));
        if (e.path().stem() != p.at("id").get<std::string>())
          continue;
        result.push_back(p);
      } catch (...) {
      }
    }
  std::sort(result.begin(), result.end(), [](const Json &a, const Json &b) {
    return a.value("name", std::string()) < b.value("name", std::string());
  });
  return result;
}
void save(const Json &input) {
  auto p = input;
  identify(p);
  id(p.at("id"));
  validate(p);
  if (p.value("name", std::string()).empty())
    fail("Enter a profile name");
  auto contents = p.dump(2) + "\n";
  if (contents.size() > 65536)
    fail("Backup profile is too large");
  write(config() / (p.at("id").get<std::string>() + ".json"), contents);
}
void schedule(const Json &p, const std::string &period) {
  id(p.at("id"));
  if (period != "off" && period != "daily" && period != "weekly")
    fail("Invalid backup schedule");
  if (p.value("delete", false) && period != "off")
    fail("Scheduled backups keep destination-only files; disable mirroring first");
  auto uuid = p.at("id").get<std::string>();
  auto name = "dome-backup-" + uuid;
  auto folder = fs::path(g_get_user_config_dir()) / "systemd/user";
  if (period == "off") {
    unschedule(uuid);
    return;
  }
  save(p);
  auto binary = g_find_program_in_path("dome-backup-job");
  if (!binary)
    fail("Install the backup job helper before enabling a schedule");
  std::string executable = binary;
  g_free(binary);
  if (executable.find_first_of("\n\r%\"\\") != std::string::npos)
    fail("Unsupported helper path");
  write(folder / (name + ".service"),
        "[Unit]\nDescription=DomeD rsync backup\n[Service]\nType=oneshot\nExecStart=\"" +
            executable + "\" " + uuid + "\nUMask=0077\nNice=10\n");
  write(folder / (name + ".timer"),
        "[Unit]\nDescription=DomeD backup schedule\n[Timer]\nOnCalendar=" + period +
            "\nPersistent=true\nRandomizedDelaySec=5m\n[Install]\nWantedBy=timers.target\n");
  userctl({"daemon-reload"});
  userctl({"enable", "--now", name + ".timer"});
}
void unschedule(const std::string &uuid) {
  id(uuid);
  auto name = "dome-backup-" + uuid;
  auto folder = fs::path(g_get_user_config_dir()) / "systemd/user";
  if (fs::exists(folder / (name + ".timer")) || fs::exists(folder / (name + ".service"))) {
    userctl({"disable", "--now", name + ".timer"});
    fs::remove(folder / (name + ".timer"));
    fs::remove(folder / (name + ".service"));
    userctl({"daemon-reload"});
  }
  // Disabling a timer must also work when its disk is unplugged or source is gone.
  for (auto p : profiles())
    if (p.at("id") == uuid) {
      p["schedule"] = "off";
      write(config() / (uuid + ".json"), p.dump(2) + "\n");
      break;
    }
}
State Job::snapshot() const {
  std::lock_guard<std::mutex> lock(mutex);
  return state;
}
Job::~Job() {
  cancel();
  if (worker.joinable())
    worker.join();
}
void Job::cancel() { stopping = true; }
void Job::start(const Json &p, bool preview) {
  if (snapshot().running)
    fail("A backup job is already running");
  auto stamp = std::to_string(g_get_real_time());
  auto args = arguments(p, preview, stamp);
  if (worker.joinable())
    worker.join();
  stopping = false;
  {
    std::lock_guard<std::mutex> lock(mutex);
    state = {};
    state.running = true;
    state.preview = preview;
  }
  worker = std::thread([this, args = std::move(args), profile = p] {
    int status = -1;
    std::string error;
    try {
      fs::create_directories(config());
      chmod(config().c_str(), 0700);
      auto uuid = profile.value("id", std::string());
      id(uuid);
      int lock = open((config() / (uuid + ".lock")).c_str(),
                      O_CREAT | O_RDWR | O_CLOEXEC | O_NOFOLLOW, 0600);
      if (lock < 0)
        fail("Cannot lock the backup profile");
      struct Release {
        int fd;
        ~Release() { close(fd); }
      } release{lock};
      if (flock(lock, LOCK_EX | LOCK_NB))
        fail("This backup profile is already running");
      validate(profile);
      status = tasks::run(
          args,
          [this](const std::string &chunk) {
            gchar *clean = g_utf8_make_valid(chunk.data(), chunk.size());
            std::string text = clean;
            g_free(clean);
            for (char &c : text)
              if (c == '\r')
                c = '\n';
              else if (static_cast<unsigned char>(c) < 32 && c != '\n' && c != '\t')
                c = '?';
            std::lock_guard<std::mutex> lock(mutex);
            state.output += text;
            if (state.output.size() > 512 * 1024)
              state.output.erase(0, state.output.size() - 384 * 1024);
          },
          stopping);
      if (status && !stopping)
        error =
            status == 23 ? "Some files could not be transferred (rsync 23). Review the log."
            : status == 24
                ? "Source files changed during backup (rsync 24). Review and run again."
                : "Rsync failed with exit status " + std::to_string(status) + ". Review the log.";
    } catch (const std::exception &e) {
      error = e.what();
    }
    std::lock_guard<std::mutex> lock(mutex);
    state.running = false;
    state.status = status;
    state.cancelled = stopping;
    state.error = error;
  });
}
} // namespace backup
