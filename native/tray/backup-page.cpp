// SPDX-License-Identifier: MIT
#include "backup-page.h"
#include "gpu-window.h"
#include <FL/Fl_Native_File_Chooser.H>
#include <FL/fl_ask.H>
#include <chrono>
#include <gio/gio.h>
using Json = backup::Json;
namespace {
std::string identifier() {
  auto value = g_uuid_string_random();
  std::string result = value;
  g_free(value);
  return result;
}
void folder(Fl_Input *input, const char *title) {
  Fl_Native_File_Chooser chooser;
  chooser.title(title);
  chooser.type(Fl_Native_File_Chooser::BROWSE_DIRECTORY);
  if (input->value()[0] == '/')
    chooser.directory(input->value());
  if (!chooser.show())
    input->value(chooser.filename());
}
} // namespace
void BackupPage::on(Fl_Widget *w, std::function<void()> f) {
  callbacks.push_back([this, f = std::move(f)] {
    try {
      f();
    } catch (const std::exception &e) {
      report(e.what());
    }
  });
  w->callback([](Fl_Widget *, void *p) { (*static_cast<std::function<void()> *>(p))(); },
              &callbacks.back());
}
void BackupPage::button(int x, int y, int w, const char *title, std::function<void()> f) {
  auto b = new Fl_Button(x, y, w, 30, title);
  on(b, std::move(f));
}
Json BackupPage::form() const {
  Json p = {{"id", uuid},
            {"name", std::string(name->value())},
            {"source", std::string(source->value())},
            {"destination", std::string(destination->value())},
            {"delete", bool(remove->value())},
            {"checksum", bool(checksum->value())},
            {"compress", bool(compress->value())},
            {"excludes", std::string(excludes->value())},
            {"schedule", period->value() == 1   ? "daily"
                         : period->value() == 2 ? "weekly"
                                                : "off"}};
  if (!loadedIdentity.is_null() && loadedDestination == destination->value())
    p["destination_identity"] = loadedIdentity;
  backup::identify(p);
  return p;
}
void BackupPage::load(const Json &p) {
  loadedIdentity = p.value("destination_identity", Json());
  loadedDestination = p.value("destination", std::string());
  uuid = p.value("id", identifier());
  name->value(p.value("name", std::string("Backup")).c_str());
  source->value(p.value("source", std::string(g_get_home_dir())).c_str());
  destination->value(p.value("destination", std::string()).c_str());
  remove->value(p.value("delete", false));
  checksum->value(p.value("checksum", false));
  compress->value(p.value("compress", false));
  excludes->value(p.value("excludes", std::string()).c_str());
  auto schedule = p.value("schedule", std::string("off"));
  period->value(schedule == "daily" ? 1 : schedule == "weekly" ? 2 : 0);
  previewed.clear();
  report("Backup profile loaded");
}
void BackupPage::refresh() {
  try {
    rows = backup::profiles();
    saved->clear();
    int index = -1;
    for (auto &p : rows) {
      auto label = p.value("name", std::string("Backup"));
      for (char &c : label)
        if (c == '/' || c == '&' || c == '\\')
          c = ' ';
      saved->add(label.c_str());
      if (p.at("id") == uuid)
        index = saved->size() - 2;
    }
    saved->value(index);
    report("Backup profiles loaded");
  } catch (const std::exception &e) {
    report(e.what());
  }
}
void BackupPage::start(bool preview) {
  try {
    auto p = form();
    backup::validate(p);
    if (!preview && p.value("delete", false)) {
      if (previewed != p.dump()) {
        report("Preview these exact settings successfully before running a mirror backup.");
        return;
      }
      if (fl_choice("Delete destination-only files? They will be retained in the destination's "
                    ".de-xr-history folder.",
                    "Cancel", "Run mirror", nullptr) != 1)
        return;
    }
    job.start(p, preview);
    jobSettings = p.dump();
    wasRunning = true;
    lastOutput.clear();
    state->copy_label(preview ? "Preview running…" : "Backup running…");
  } catch (const std::exception &e) {
    report(e.what());
  }
}
void BackupPage::restore() {
  if (job.snapshot().running) {
    report("Wait for the current backup or cancel it first.");
    return;
  }
  try {
    auto p = form();
    Fl_Native_File_Chooser chooser;
    chooser.title("Choose a directory to restore into");
    chooser.type(Fl_Native_File_Chooser::BROWSE_DIRECTORY);
    if (source->value()[0] == '/')
      chooser.directory(source->value());
    if (chooser.show())
      return;
    std::string target = chooser.filename();
    if (fl_choice("Restore the latest backed-up files into the selected directory? Existing "
                  "changed files there will be retained in .de-xr-history.",
                  "Cancel", "Restore", nullptr) != 1)
      return;
    p["source"] = p.at("destination");
    p["destination"] = target;
    p["delete"] = false;
    p["schedule"] = "off";
    p.erase("destination_identity");
    backup::identify(p);
    job.start(p, false);
    jobSettings = p.dump();
    wasRunning = true;
    lastOutput.clear();
    state->copy_label("Restore running…");
  } catch (const std::exception &e) {
    report(e.what());
  }
}
void BackupPage::tick() {
  if (pending.valid() && pending.wait_for(std::chrono::seconds(0)) == std::future_status::ready) {
    try {
      pending.get();
      refresh();
      report("Backup schedule updated");
    } catch (const std::exception &e) {
      report(e.what());
    }
  }
  auto current = job.snapshot();
  if (current.output != lastOutput) {
    lastOutput = current.output;
    log->value(lastOutput.c_str());
  }
  if (wasRunning && !current.running) {
    wasRunning = false;
    if (current.preview && !current.cancelled && current.status == 0)
      previewed = jobSettings;
    else if (current.preview)
      previewed.clear();
    std::string message = current.cancelled        ? "Job cancelled"
                          : !current.error.empty() ? current.error
                          : current.preview        ? "Preview completed — no files changed"
                                                   : "Backup / restore completed";
    state->copy_label(message.c_str());
    report(message);
  }
}
BackupPage::BackupPage(int X, int Y, int W, int H, std::function<void(const std::string &)> status)
    : Fl_Group(X, Y, W, H, "Backups"), uuid(identifier()), report(std::move(status)) {
  saved = new Fl_Choice(X + 150, Y + 16, 320, 30, "Saved profile:");
  on(saved, [this] {
    int i = saved->value();
    if (i >= 0 && i < int(rows.size()))
      load(rows[i]);
  });
  button(X + 490, Y + 16, 90, "New", [this] { load(Json::object()); });
  button(X + 595, Y + 16, 110, "Save profile", [this] {
    try {
      auto p = form();
      backup::identify(p, true);
      backup::save(p);
      loadedIdentity = p.value("destination_identity", Json());
      loadedDestination = p.at("destination");
      refresh();
      report("Backup profile saved");
    } catch (const std::exception &e) {
      report(e.what());
    }
  });
  name = new Fl_Input(X + 150, Y + 62, W - 180, 30, "Name:");
  source = new Fl_Input(X + 150, Y + 104, W - 295, 30, "Source:");
  button(X + W - 130, Y + 104, 100, "Browse…", [this] { folder(source, "Source directory"); });
  destination = new Fl_Input(X + 150, Y + 146, W - 295, 30, "Backup to:");
  button(X + W - 130, Y + 146, 100, "Browse…", [this] {
    folder(destination, "Backup destination directory");
    loadedIdentity = Json();
  });
  source->tooltip("Directory contents are copied, including hidden files. Remote paths use "
                  "user@host:/absolute/path.");
  destination->tooltip(
      "Changed files are versioned in .de-xr-history. SSH uses configured keys and known hosts.");
  remove = new Fl_Check_Button(X + 20, Y + 190, 400, 28, "Mirror deletions (preview required)");
  checksum = new Fl_Check_Button(X + 430, Y + 190, 200, 28, "Verify by checksum");
  compress = new Fl_Check_Button(X + 650, Y + 190, 230, 28, "Compress transfer");
  excludes = new Fl_Multiline_Input(X + 150, Y + 232, 350, 72, "Exclude patterns:");
  excludes->tooltip(
      "One rsync exclude pattern per line; no shell commands. .de-xr-history is always excluded.");
  period = new Fl_Choice(X + 625, Y + 234, 250, 30, "Schedule:");
  period->add("Off|Daily|Weekly");
  period->value(0);
  button(X + 625, Y + 274, 250, "Apply saved schedule", [this] {
    if (pending.valid()) {
      report("Wait for the schedule operation.");
      return;
    }
    if (period->value() == 0) {
      auto id = uuid;
      pending = std::async(std::launch::async, [id] { backup::unschedule(id); });
    } else {
      auto p = form();
      pending = std::async(std::launch::async, [p] { backup::schedule(p, p.at("schedule")); });
    }
    report("Updating backup schedule…");
  });
  button(X + 20, Y + 326, 125, "Preview", [this] { start(true); });
  button(X + 155, Y + 326, 140, "Back up now", [this] { start(false); });
  button(X + 305, Y + 326, 125, "Restore…", [this] { restore(); });
  button(X + 440, Y + 326, 100, "Cancel job", [this] { job.cancel(); });
  state = new Fl_Box(X + 555, Y + 326, W - 575, 30, "Ready — backups run as your user");
  state->align(FL_ALIGN_LEFT | FL_ALIGN_INSIDE | FL_ALIGN_WRAP);
  log = new Fl_Multiline_Output(X + 20, Y + 370, W - 40, H - 388);
  log->wrap(1);
  end();
  load(Json::object());
  refresh();
}
