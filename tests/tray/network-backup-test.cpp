// SPDX-License-Identifier: MIT
#include "backup.h"
#include "network.h"
#include "process.h"
#include <cassert>
#include <fcntl.h>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <sys/file.h>
#include <sys/stat.h>
#include <thread>
#include <unistd.h>
namespace fs = std::filesystem;
using Json = nlohmann::json;
template <class F> void rejects(F f) {
  bool rejected = false;
  try {
    f();
  } catch (const std::exception &) {
    rejected = true;
  }
  assert(rejected);
}
std::string read(const fs::path &p) {
  std::ifstream f(p);
  return {std::istreambuf_iterator<char>(f), {}};
}
backup::State done(backup::Job &job) {
  auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(15);
  while (job.snapshot().running) {
    assert(std::chrono::steady_clock::now() < deadline);
    std::this_thread::sleep_for(std::chrono::milliseconds(10));
  }
  return job.snapshot();
}
int main(int argc, char **argv) {
  if (argc == 2 && std::string(argv[1]) == "--snapshot") {
    auto state = network::snapshot();
    std::cout << "PASS read-only NetworkManager query: " << state["devices"].size() << " devices, "
              << state["profiles"].size() << " profiles\n";
    return 0;
  }
  char temporary[] = "/tmp/dome-network-backup-XXXXXX";
  assert(mkdtemp(temporary));
  fs::path root = temporary;
  setenv("XDG_CONFIG_HOME", (root / "config").c_str(), 1);
  Json request = {{"ssid", "4365736b79206e6574776f726b"},
                  {"name", "Český network"},
                  {"security", "wpa-psk"},
                  {"password", "not-a-real-password"}};
  auto wifi = network::wifiConnection(request);
  assert(nm_connection_verify(wifi, nullptr));
  auto security = nm_connection_get_setting_wireless_security(wifi);
  assert(std::string(nm_setting_wireless_security_get_key_mgmt(security)) == "wpa-psk");
  auto uuid = std::string(nm_connection_get_uuid(wifi));
  auto wireless = nm_connection_get_setting_wireless(wifi);
  auto ssid = g_bytes_ref(nm_setting_wireless_get_ssid(wireless));
  network::editProfile(wifi, {{"name", "Renamed"}, {"autoconnect", false}});
  assert(uuid == nm_connection_get_uuid(wifi));
  assert(
      g_bytes_equal(ssid, nm_setting_wireless_get_ssid(nm_connection_get_setting_wireless(wifi))));
  assert(std::string(nm_setting_wireless_security_get_psk(
             nm_connection_get_setting_wireless_security(wifi))) == "not-a-real-password");
  auto ip = nm_connection_get_setting_ip4_config(wifi);
  auto route = nm_ip_route_new(AF_INET, "10.20.0.0", 16, "192.168.1.1", 10, nullptr);
  assert(route);
  nm_setting_ip_config_add_route(ip, route);
  nm_ip_route_unref(route);
  Json v4 = {{"method", "manual"},
             {"address", "192.168.1.5/24"},
             {"gateway", "192.168.1.1"},
             {"dns", "1.1.1.1, 9.9.9.9"},
             {"ignore_auto_dns", true}};
  network::editProfile(wifi, {{"name", "Renamed"}, {"autoconnect", false}, {"ipv4", v4}});
  assert(nm_setting_ip_config_get_num_routes(ip) == 1);
  assert(nm_setting_ip_config_get_num_addresses(ip) == 1);
  assert(nm_setting_ip_config_get_num_dns(ip) == 2);
  assert(nm_setting_ip_config_get_ignore_auto_dns(ip));
  auto copy = nm_simple_connection_new_clone(wifi);
  v4["dns"] = "not-an-address";
  rejects([&] { network::editProfile(copy, {{"name", "Invalid"}, {"ipv4", v4}}); });
  g_object_unref(copy);
  assert(nm_setting_ip_config_get_num_dns(ip) == 2);
  g_bytes_unref(ssid);
  g_object_unref(wifi);
  request["password"] = "short";
  rejects([&] { g_object_unref(network::wifiConnection(request)); });
  request["security"] = "legacy";
  rejects([&] { g_object_unref(network::wifiConnection(request)); });
  request["security"] = "open";
  wifi = network::wifiConnection(request);
  assert(!nm_connection_get_setting_wireless_security(wifi));
  g_object_unref(wifi);
  request["ssid"] = std::string(66, 'a');
  rejects([&] { g_object_unref(network::wifiConnection(request)); });
  std::cout << "PASS Wi-Fi settings, validation, profile identity, secrets and unedited route "
               "preservation\n";
  auto source = root / "source with spaces", destination = root / "backup 日本語";
  fs::create_directories(source);
  fs::create_directories(destination);
  std::ofstream(source / "hello.txt") << "original";
  std::ofstream(source / ".private") << "hidden";
  fs::create_symlink("hello.txt", source / "symbolic");
  std::ofstream(destination / "keep.txt") << "keep";
  auto value = g_uuid_string_random();
  std::string id = value;
  g_free(value);
  Json p = {{"id", id},
            {"name", "Test backup"},
            {"source", source.string()},
            {"destination", destination.string()},
            {"delete", false},
            {"schedule", "off"}};
  backup::identify(p);
  backup::save(p);
  assert(backup::profiles().size() == 1);
  struct stat info;
  assert(!stat((root / "config/dome/backups" / (id + ".json")).c_str(), &info));
  assert((info.st_mode & 0777) == 0600);
  backup::Job job;
  job.start(p, true);
  auto result = done(job);
  assert(result.status == 0 && result.preview);
  assert(!fs::exists(destination / "hello.txt"));
  job.start(p, false);
  result = done(job);
  assert(result.status == 0);
  assert(read(destination / "hello.txt") == "original");
  assert(read(destination / ".private") == "hidden");
  assert(fs::is_symlink(destination / "symbolic"));
  assert(fs::exists(destination / "keep.txt"));
  std::ofstream(source / "hello.txt") << "new and longer contents";
  job.start(p, false);
  assert(done(job).status == 0);
  assert(read(destination / "hello.txt") == "new and longer contents");
  bool history = false;
  for (auto &e : fs::recursive_directory_iterator(destination / ".de-xr-history"))
    if (e.path().filename() == "hello.txt" && read(e.path()) == "original")
      history = true;
  assert(history);
  p["delete"] = true;
  job.start(p, true);
  assert(done(job).status == 0);
  assert(fs::exists(destination / "keep.txt"));
  job.start(p, false);
  assert(done(job).status == 0);
  assert(!fs::exists(destination / "keep.txt"));
  bool retained = false;
  for (auto &e : fs::recursive_directory_iterator(destination / ".de-xr-history"))
    if (e.path().filename() == "keep.txt" && read(e.path()) == "keep")
      retained = true;
  assert(retained);
  p["delete"] = false;
  auto invalid = p;
  invalid["destination"] = source.string();
  invalid.erase("destination_identity");
  rejects([&] { backup::validate(invalid); });
  fs::create_directory(source / "nested");
  invalid["destination"] = (source / "nested").string();
  rejects([&] { backup::validate(invalid); });
  fs::create_directory_symlink(source, root / "alias");
  invalid["destination"] = (root / "alias").string();
  rejects([&] { backup::validate(invalid); });
  invalid = p;
  invalid["destination_identity"]["filesystem"] = Json::array({123456, 654321});
  rejects([&] { backup::validate(invalid); });
  invalid = p;
  invalid["destination"] = "--delete";
  rejects([&] { backup::validate(invalid); });
  invalid = p;
  invalid["destination"] = "user@example.invalid:/backup/space ; literal";
  invalid.erase("destination_identity");
  auto args = backup::arguments(invalid, true, "12345");
  assert(args.back() == "user@example.invalid:/backup/space ; literal/");
  assert(std::find(args.begin(), args.end(), "--secluded-args") != args.end());
  invalid["source"] = "other@example.invalid:/source";
  rejects([&] { backup::validate(invalid); });
  invalid = p;
  invalid["id"] = "../escape";
  rejects([&] { backup::save(invalid); });
  auto lockPath = root / "config/dome/backups" / (id + ".lock");
  int lock = open(lockPath.c_str(), O_RDWR | O_CREAT, 0600);
  assert(lock >= 0 && !flock(lock, LOCK_EX | LOCK_NB));
  job.start(p, false);
  assert(done(job).status != 0);
  close(lock);
  std::atomic_bool cancel{false};
  auto began = std::chrono::steady_clock::now();
  std::thread stopper([&] {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
    cancel = true;
  });
  int status = tasks::run({"sh", "-c", "sleep 30 & wait"}, [](const std::string &) {}, cancel, 5);
  stopper.join();
  assert(status != 0 && std::chrono::steady_clock::now() - began < std::chrono::seconds(4));
  std::cout << "PASS real rsync preview/copy, Unicode, hidden files, symlinks, version history, "
               "safe deletion, profile permissions, mount identity, overlap rejection, per-profile "
               "locks and child-group cancellation\n";
  // Scheduling is tested against a private executable, never the user's service manager.
  auto bin = root / "bin";
  fs::create_directories(bin);
  std::ofstream(bin / "systemctl") << "#!/bin/sh\nexit 0\n";
  std::ofstream(bin / "dome-backup-job") << "#!/bin/sh\nexit 0\n";
  chmod((bin / "systemctl").c_str(), 0700);
  chmod((bin / "dome-backup-job").c_str(), 0700);
  auto path = bin.string() + ":" + getenv("PATH");
  setenv("PATH", path.c_str(), 1);
  p["schedule"] = "daily";
  backup::schedule(p, "daily");
  auto unit = root / "config/systemd/user" / ("dome-backup-" + id + ".timer");
  assert(read(unit).find("OnCalendar=daily") != std::string::npos);
  assert(read(unit).find("Persistent=true") != std::string::npos);
  invalid = p;
  invalid["delete"] = true;
  rejects([&] { backup::schedule(invalid, "daily"); });
  fs::rename(destination, root / "disconnected-destination");
  p["schedule"] = "off";
  backup::schedule(p, "off");
  assert(!fs::exists(unit));
  assert(backup::profiles()[0].at("schedule") == "off");
  std::cout << "PASS isolated daily timer generation and deletion-policy enforcement\n";
  fs::remove_all(root);
}
