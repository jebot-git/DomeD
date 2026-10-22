// SPDX-License-Identifier: MIT
#include "desktop-menu.h"
#include "gpu-window.h"
#include "notifier.h"
#include "services.h"
#include "keyboard-flags.h"
#include <FL/Fl_Box.H>
#include <FL/Fl_Button.H>
#include <FL/Fl_Choice.H>
#include <FL/Fl_Hold_Browser.H>
#include <FL/Fl_Input.H>
#include <FL/Fl_Menu_Bar.H>
#include <FL/Fl_Multiline_Input.H>
#include <FL/Fl_Multiline_Output.H>
#include <FL/Fl_Native_File_Chooser.H>
#include <FL/Fl_Return_Button.H>
#include <FL/Fl_Scroll.H>
#include <FL/Fl_Secret_Input.H>
#include <FL/Fl_Tabs.H>
#include <FL/fl_ask.H>
#include <chrono>
#include <cstring>
#include <deque>
#include <functional>
#include <future>
#include <map>
#include <sys/resource.h>
using services::Json;
class FlagImage : public Fl_RGB_Image {
public:
  explicit FlagImage(const Fl_RGB_Image &source)
      : Fl_RGB_Image(new uchar[source.w() * source.h() * source.d()],
                     source.w(), source.h(), source.d()) {
    alloc_array = 1;
    auto stride = source.ld() ? source.ld() : source.w() * source.d();
    for (int y = 0; y < h(); ++y)
      memcpy(const_cast<uchar *>(array) + y * w() * d(),
             source.array + y * stride, w() * d());
  }
  void draw(int x, int y, int width, int height, int cx = 0, int cy = 0) override {
    // FLTK's GL widget driver has no Fl_RGB_Image drawing implementation.
    auto window = dynamic_cast<Fl_Gl_Window *>(Fl_Window::current());
    float scale = window ? window->pixels_per_unit() : 1.f;
    glPushAttrib(GL_COLOR_BUFFER_BIT | GL_PIXEL_MODE_BIT | GL_CURRENT_BIT | GL_ENABLE_BIT);
    glEnable(GL_BLEND);
    glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
    glPixelZoom(scale, -scale);
    gl_draw_image(array + (cy * w() + cx) * d(), x, y,
                  std::min(width, w() - cx), std::min(height, h() - cy), d(), w() * d());
    glPopAttrib();
  }
};
class Manager : public dx::GpuWindow {
  Fl_Menu_Bar menu{0, 0, 940, 28};
  DesktopMenu exported;
  Fl_Tabs tabs{12, 38, 916, 566};
  Fl_Group *trayTab, *deviceTab, *inputTab, *walletTab;
  Fl_Box status{12, 612, 916, 36};
  Fl_Hold_Browser *trayList, *trayRows, *deviceList, *plugins, *available,
      *enabled, *folderList, *entryList, *configList;
  Fl_Multiline_Output *deviceInfo;
  Fl_Choice *groups, *configTarget;
  Fl_Input *layout, *entryName;
  Fl_Secret_Input *password;
  Json trayData = Json::array(), rows = Json::array(),
       deviceData = Json::array(), inputData, groupData,
       entryData = Json::array(), configuration;
  Json selectedTray;
  std::vector<int> parents{0};
  std::string selectedDevice, walletName, selectedFolder, loadedEntry,
      configUri, requestedInputConfig;
  std::map<std::string, std::unique_ptr<Fl_RGB_Image>> flags;
  unsigned flagForeground = 0;
  Fl_Image *flag(const std::string &method, const std::string &language = "") {
    auto country = dx_flags::country(method, language);
    unsigned char r, g, b;
    Fl::get_color(FL_FOREGROUND_COLOR, r, g, b);
    char tint[8];
    snprintf(tint, sizeof(tint), "#%02x%02x%02x", r, g, b);
    auto &image = flags[country + tint];
    if (!image) {
      auto svg = dx_flags::svg(country, tint);
      Fl_SVG_Image source(nullptr, svg.c_str());
      source.resize(24, 24);
      source.normalize();
      // The native GL browser driver consumes RGB images directly.
      image = std::make_unique<FlagImage>(source);
    }
    return image.get();
  }
  int handle = -1;
  Fl_Choice *walletList;
  bool quitting = false;
  std::future<Json> pending;
  std::function<void(Json)> completed;
  std::deque<std::function<void()>> callbacks;
  double lastRefresh = 0;
  template <class W> void on(W *widget, std::function<void()> fn) {
    callbacks.push_back(std::move(fn));
    widget->callback(
        [](Fl_Widget *, void *p) {
          (*static_cast<std::function<void()> *>(p))();
        },
        &callbacks.back());
  }
  Fl_Button *button(int x, int y, int w, const char *label,
                    std::function<void()> fn) {
    auto b = new Fl_Button(x, y, w, 30, label);
    on(b, std::move(fn));
    return b;
  }
  static std::string selected(Fl_Hold_Browser *browser) {
    auto p = browser->text(browser->value());
    return p ? p : "";
  }
  void message(const std::string &s) {
    status.copy_label(s.c_str());
    status.redraw();
  }
  void submit(std::function<Json()> work, std::function<void(Json)> done = {}) {
    if (pending.valid()) {
      message("Please wait for the current operation.");
      return;
    }
    message("Working…");
    completed = std::move(done);
    pending = std::async(std::launch::async, [work] { return work(); });
  }
  void command(std::function<void()> work, std::function<void()> done = {}) {
    submit(
        [work] {
          work();
          return Json();
        },
        [this, done](Json) {
          message("Done");
          if (done)
            done();
        });
  }
  static void tick(void *p) {
    auto self = static_cast<Manager *>(p);
    auto foreground = Fl::get_color(FL_FOREGROUND_COLOR);
    if (foreground != self->flagForeground && self->inputData.is_object()) {
      self->flagForeground = foreground;
      int row = 0;
      for (auto &method : self->inputData["methods"])
        self->available->icon(++row, self->flag(method.at("id"), method.value("language", "")));
      if (self->groupData.is_object()) {
        row = 0;
        for (auto &method : self->groupData["methods"]) {
          std::string language;
          for (auto &available : self->inputData["methods"])
            if (available["id"] == method["id"]) language = available.value("language", "");
          self->enabled->icon(++row, self->flag(method.at("id"), language));
        }
      }
      self->redraw();
    }
    while (g_main_context_pending(nullptr))
      g_main_context_iteration(nullptr, FALSE);
    if (self->pending.valid() && self->pending.wait_for(std::chrono::seconds(
                                     0)) == std::future_status::ready) {
      try {
        auto result = self->pending.get();
        self->message("Ready");
        auto done = std::move(self->completed);
        if (done)
          done(std::move(result));
      } catch (const std::exception &e) {
        self->message(e.what());
      }
    }
    if (!self->pending.valid() && !self->requestedInputConfig.empty()) {
      auto uri = std::move(self->requestedInputConfig);
      self->requestedInputConfig.clear();
      if (uri == "fcitx://config/inputmethod/@current") {
        auto method = self->inputData.is_object()
                          ? self->inputData.value("current", std::string()) : std::string();
        if (method.empty()) self->message("No active input method.");
        else self->loadInputConfig("fcitx://config/inputmethod/" + method);
      } else self->loadInputConfig(uri);
    }
    Fl::repeat_timeout(.04, tick, p);
  }
  void refreshTray() {
    submit(services::trayItems, [this](Json data) {
      trayData = std::move(data);
      trayList->clear();
      for (auto &item : trayData)
        trayList->add(
            item.value("Title", item.value("Id", std::string("Application")))
                .c_str());
    });
  }
  void openTray() {
    int index = trayList->value() - 1;
    if (index < 0 || size_t(index) >= trayData.size())
      return;
    selectedTray = trayData[index];
    auto native =
        services::frontend(selectedTray.value("Id", std::string()) + " " +
                           selectedTray.value("Title", std::string()));
    if (!native.empty()) {
      showPage(native);
      return;
    }
    parents = {0};
    loadTrayMenu(0);
  }
  void loadTrayMenu(int parent) {
    auto item = selectedTray;
    submit([item, parent] { return services::trayMenu(item, parent); },
           [this](Json data) {
             rows = std::move(data);
             trayRows->clear();
             for (auto &row : rows) {
               auto label = row.value("separator", false)
                                ? std::string("────────")
                                : row.value("label", std::string());
               label.erase(std::remove(label.begin(), label.end(), '_'),
                           label.end());
               if (row.value("checkable", false))
                 label =
                     (row.value("checked", false) ? "[x] " : "[ ] ") + label;
               if (row.value("submenu", false))
                 label += "  ›";
               if (!row.value("enabled", true))
                 label += " (unavailable)";
               trayRows->add(label.c_str());
             }
           });
  }
  void trayAction() {
    int i = trayRows->value() - 1;
    if (i < 0 || size_t(i) >= rows.size())
      return;
    auto row = rows[i];
    if (!row.value("enabled", true) || row.value("separator", false))
      return;
    if (row.value("submenu", false)) {
      parents.push_back(row.at("id"));
      loadTrayMenu(parents.back());
    } else {
      auto item = selectedTray;
      command([item, row] { services::trayEvent(item, row.at("id")); },
              [this] { loadTrayMenu(parents.back()); });
    }
  }
  void refreshDevices() {
    submit(services::devices, [this](Json data) {
      deviceData = std::move(data);
      deviceList->clear();
      int selected = 0;
      for (size_t i = 0; i < deviceData.size(); ++i) {
        auto &d = deviceData[i];
        auto text =
            d.value("name", d.at("id").get<std::string>()) +
            (d.value("isReachable", false) ? " — online" : " — offline") +
            (d.value("isPaired", false) ? " — paired" : "");
        deviceList->add(text.c_str());
        if (d.at("id") == selectedDevice)
          selected = i + 1;
      }
      deviceList->value(selected);
    });
  }
  void selectDevice() {
    int i = deviceList->value() - 1;
    if (i < 0 || size_t(i) >= deviceData.size())
      return;
    selectedDevice = deviceData[i].at("id");
    auto id = selectedDevice;
    submit([id] { return services::device(id); },
           [this](Json d) {
             std::string text =
                 d.value("name", selectedDevice) + "\n" +
                 (d.value("isPaired", false) ? "Paired" : "Not paired") + "\n" +
                 (d.value("isReachable", false) ? "Connected" : "Offline");
             if (d.value("isPairRequestedByPeer", false))
               text += "\nPairing requested by this device";
             if (d.contains("verificationKey"))
               text +=
                   "\nVerification: " + d["verificationKey"].get<std::string>();
             if (d.contains("battery"))
               text += "\nBattery: " +
                       std::to_string(d["battery"].value("charge", 0)) + "%";
             deviceInfo->value(text.c_str());
             plugins->clear();
             for (auto &p : d.value("plugins", Json::array()))
               plugins->add(p.get<std::string>().c_str());
           });
  }
  void deviceAction(const std::string &action, std::string value = {}) {
    if (selectedDevice.empty() && action != "refresh" && action != "rename") {
      message("Select a device first.");
      return;
    }
    auto id = selectedDevice;
    command([id, action, value] { services::deviceAction(id, action, value); },
            [this] { refreshDevices(); });
  }
  void refreshInput() {
    submit(services::methods, [this](Json data) {
      inputData = std::move(data);
      available->clear();
      groups->clear();
      configTarget->clear();
      configTarget->add("Global settings");
      for (auto &m : inputData["methods"]) {
        available->add((m.at("name").get<std::string>() + " (" +
                        m.at("id").get<std::string>() + ")")
                           .c_str());
        available->icon(available->size(), flag(m.at("id"), m.value("language", "")));
      }
      int i = 0, chosen = 0;
      for (auto &g : inputData["groups"]) {
        groups->add(g.get<std::string>().c_str());
        if (g == inputData["group"])
          chosen = i;
        ++i;
      }
      groups->value(chosen);
      configTarget->add("Selected input method");
      for (auto &addon : inputData["addons"])
        configTarget->add(addon.at("name").get<std::string>().c_str());
      configTarget->value(0);
      message("Current input method: " +
              inputData.value("current", std::string()));
      loadGroup();
    });
  }
  std::string group() {
    auto p = groups->text();
    return p ? p : "";
  }
  void loadGroup() {
    auto name = group();
    if (name.empty())
      return;
    submit([name] { return services::inputGroup(name); },
           [this](Json d) {
             groupData = std::move(d);
             layout->value(groupData.value("layout", std::string()).c_str());
             updateGroup();
           });
  }
  void updateGroup() {
    enabled->clear();
    for (auto &m : groupData["methods"]) {
      enabled->add((m.at("id").get<std::string>() + "  " +
                    m.value("layout", std::string()))
                       .c_str());
      std::string language;
      for (auto &method : inputData["methods"])
        if (method["id"] == m["id"]) language = method.value("language", "");
      enabled->icon(enabled->size(), flag(m.at("id"), language));
    }
  }
  void saveGroup() {
    if (groupData.is_null())
      return;
    auto name = group(), l = std::string(layout->value());
    auto ms = groupData["methods"];
    command([name, l, ms] { services::saveGroup(name, l, ms); });
  }
  void configureInput() {
    configUri = "fcitx://config/global";
    if (configTarget->value() == 1) {
      int i = available->value() - 1;
      if (i < 0 || size_t(i) >= inputData["methods"].size()) {
        message("Select an input method first.");
        return;
      }
      configUri = "fcitx://config/inputmethod/" +
                  inputData["methods"][i]["id"].get<std::string>();
    }
    if (configTarget->value() >= 2)
      configUri = "fcitx://config/addon/" +
                  inputData["addons"][configTarget->value() - 2]["id"]
                      .get<std::string>();
    loadInputConfig(configUri);
  }
  void loadInputConfig(const std::string &uri) {
    configUri = uri;
    configTarget->value(0);
    for (size_t i = 0; i < inputData["methods"].size(); ++i)
      if (uri == "fcitx://config/inputmethod/" + inputData["methods"][i]["id"].get<std::string>()) {
        available->value(i + 1);
        configTarget->value(1);
      }
    for (size_t i = 0; i < inputData["addons"].size(); ++i)
      if (uri == "fcitx://config/addon/" + inputData["addons"][i]["id"].get<std::string>())
        configTarget->value(i + 2);
    submit([uri] { return services::inputConfig(uri); },
           [this](Json data) {
             configuration = std::move(data);
             configList->clear();
             for (auto it = configuration.begin(); it != configuration.end();
                  ++it)
               configList->add(
                   (it.key() + " = " + it.value().get<std::string>()).c_str());
           });
  }
  void editConfig() {
    int i = configList->value() - 1;
    if (i < 0 || size_t(i) >= configuration.size())
      return;
    auto it = configuration.begin();
    std::advance(it, i);
    auto value =
        fl_input("%s", it.value().get<std::string>().c_str(), it.key().c_str());
    if (!value)
      return;
    it.value() = value;
    configList->text(i + 1, (it.key() + " = " + std::string(value)).c_str());
  }
  void refreshWallets() {
    submit(services::wallets, [this](Json names) {
      walletList->clear();
      for (auto &name : names)
        walletList->add(name.get<std::string>().c_str());
      if (names.size())
        walletList->value(0);
    });
  }
  void clearSecret() {
    password->value("");
    loadedEntry.clear();
  }
  void openWallet(std::string requested = {}) {
    auto name = requested.empty() ? walletList->text() : requested.c_str();
    if (!name)
      return;
    walletName = name;
    auto n = walletName;
    clearSecret();
    submit(
        [n] {
          int h = services::openWallet(n);
          return Json{{"handle", h}, {"folders", services::folders(h)}};
        },
        [this](Json d) {
          handle = d.at("handle");
          folderList->clear();
          entryList->clear();
          for (auto &f : d["folders"])
            folderList->add(f.get<std::string>().c_str());
        });
  }
  void loadFolder() {
    if (handle < 0)
      return;
    selectedFolder = selected(folderList);
    auto h = handle;
    auto f = selectedFolder;
    clearSecret();
    submit([h, f] { return services::entries(h, f); },
           [this](Json data) {
             entryData = std::move(data);
             entryList->clear();
             for (auto &e : entryData) {
               int type = e.at("type");
               entryList->add(
                   (e.at("key").get<std::string>() + (type == 1 ? " (password)"
                                                      : type == 2 ? " (stream)"
                                                                  : " (map)"))
                       .c_str());
             }
           });
  }
  void selectedEntry() {
    clearSecret();
    int i = entryList->value() - 1;
    if (i >= 0 && size_t(i) < entryData.size())
      entryName->value(entryData[i]["key"].get<std::string>().c_str());
  }
  void walletAction(const std::string &action, std::string key = {},
                    std::string value = {}) {
    if (handle < 0) {
      message("Open a wallet first.");
      return;
    }
    auto h = handle;
    auto f = selectedFolder;
    command([action, h, f, key,
             value] { services::walletAction(action, h, f, key, value); },
            [this, action] {
              clearSecret();
              if (action == "lock") {
                handle = -1;
                folderList->clear();
                entryList->clear();
                message("Wallet locked or access released.");
              } else if (action == "create-folder" || action == "remove-folder")
                openWallet();
              else
                loadFolder();
            });
  }
  void readEntry() {
    if (handle < 0)
      return;
    int i = entryList->value() - 1;
    if (i < 0 || size_t(i) >= entryData.size())
      return;
    if (entryData[i]["type"] != 1) {
      message(
          "This entry contains a map or binary stream; use the data editor.");
      return;
    }
    auto h = handle;
    auto f = selectedFolder;
    auto key = entryData[i]["key"].get<std::string>();
    submit([h, f, key] { return Json(services::readPassword(h, f, key)); },
           [this, key](Json value) {
             loadedEntry = key;
             password->value(value.get<std::string>().c_str());
             message("Password loaded and masked. Clear it when finished.");
           });
  }
  void dataEditor(bool read) {
    if (handle < 0 || selectedFolder.empty()) {
      message("Open a wallet and select a folder first.");
      return;
    }
    auto key = std::string(entryName->value());
    if (key.empty()) {
      message("Select or name an entry first.");
      return;
    }
    auto edit = [this, key](Json data) {
      dx::GpuWindow dialog(700, 470, "Wallet entry data");
      Fl_Choice type(110, 20, 250, 30, "Type:");
      type.add("Password");
      type.add("Binary (hexadecimal)");
      type.add("Map (JSON strings)");
      type.value(data.value("type", 3) - 1);
      Fl_Multiline_Input content(20, 75, 660, 310);
      content.value(data.value("data", std::string("{}")).c_str());
      Fl_Button cancel(390, 410, 130, 32, "Cancel"),
          save(540, 410, 130, 32, "Save");
      bool accepted = false;
      auto callbackCount = callbacks.size();
      on(&save, [&] {
        if (fl_choice("Write this data to the selected wallet entry?", "Cancel",
                      "Save", nullptr) == 1) {
          accepted = true;
          dialog.hide();
        }
      });
      on(&cancel, [&] { dialog.hide(); });
      dialog.end();
      dialog.set_modal();
      dx::style(&dialog);
      dialog.show();
      while (dialog.shown())
        Fl::wait();
      if (accepted) {
        auto h = handle;
        auto folder = selectedFolder;
        Json result = {{"type", type.value() + 1}, {"data", content.value()}};
        content.value("");
        command([h, folder, key,
                 result] { services::writeData(h, folder, key, result); },
                [this] { loadFolder(); });
      }
      content.value("");
      while (callbacks.size() > callbackCount)
        callbacks.pop_back();
    };
    if (read) {
      int i = entryList->value() - 1;
      if (i >= 0 && size_t(i) < entryData.size() && entryData[i]["type"] == 1) {
        message("Use Read password for masked password viewing.");
        return;
      }
      auto h = handle;
      auto folder = selectedFolder;
      submit([h, folder, key] { return services::readData(h, folder, key); },
             edit);
    } else
      edit({{"type", 3}, {"data", "{}"}});
  }

public:
  Manager()
      : dx::GpuWindow(940, 660, "DomeD tray applications"),
        exported(*this, menu) {
    begin();
    add(menu);
    add(tabs);
    add(status);
    menu.add(
        "&File/&Refresh", FL_CTRL + 'r',
        [](Fl_Widget *, void *p) { ((Manager *)p)->refresh(); }, this);
    menu.add(
        "&File/&Close", FL_CTRL + 'w',
        [](Fl_Widget *, void *p) { ((Manager *)p)->close(); }, this);
    menu.add(
        "&File/&Quit", FL_CTRL + 'q',
        [](Fl_Widget *, void *p) {
          auto s = (Manager *)p;
          s->quitting = true;
          s->hide();
        },
        this);
    menu.add(
        "&View/&Tray", FL_ALT + '1',
        [](Fl_Widget *, void *p) { ((Manager *)p)->showPage("tray"); }, this);
    menu.add(
        "&View/&Devices", FL_ALT + '2',
        [](Fl_Widget *, void *p) { ((Manager *)p)->showPage("devices"); },
        this);
    menu.add(
        "&View/&Input methods", FL_ALT + '3',
        [](Fl_Widget *, void *p) { ((Manager *)p)->showPage("input"); }, this);
    menu.add(
        "&View/&Wallets", FL_ALT + '4',
        [](Fl_Widget *, void *p) { ((Manager *)p)->showPage("wallets"); },
        this);
    menu.add("&Input methods/&Configure…", FL_CTRL + ',',
             [](Fl_Widget *, void *p) { ((Manager *)p)->showPage("input"); }, this);
    menu.add("&Input methods/&Global options…", 0,
             [](Fl_Widget *, void *p) { ((Manager *)p)->showPage("input:fcitx://config/global"); }, this);
    menu.add("&Input methods/&Current method…", 0,
             [](Fl_Widget *, void *p) {
               ((Manager *)p)->showPage("input:fcitx://config/inputmethod/@current");
             }, this);
    menu.add("&Input methods/&Reload configuration", 0,
             [](Fl_Widget *, void *p) {
               auto self = static_cast<Manager *>(p);
               self->command([] { services::inputAction("reload", ""); });
             }, this);
    menu.add(
        "&Wallet/&New wallet…", 0,
        [](Fl_Widget *, void *p) {
          auto self = static_cast<Manager *>(p);
          auto name = fl_input("New wallet name:");
          if (name && *name)
            self->openWallet(name);
        },
        this);
    menu.add(
        "&Wallet/&Delete wallet…", 0,
        [](Fl_Widget *, void *p) {
          auto self = static_cast<Manager *>(p);
          auto value = self->walletList->text();
          if (!value)
            return;
          std::string name = value;
          if (fl_choice(
                  "Permanently delete the selected wallet and all entries?",
                  "Cancel", "Delete", nullptr) == 1)
            self->command([name] { services::walletLifecycle("delete", name); },
                          [self] {
                            self->handle = -1;
                            self->folderList->clear();
                            self->entryList->clear();
                            self->clearSecret();
                            self->refreshWallets();
                          });
        },
        this);
    menu.add(
        "&Wallet/Change wallet &password…", 0,
        [](Fl_Widget *, void *p) {
          auto self = static_cast<Manager *>(p);
          auto value = self->walletList->text();
          if (!value)
            return;
          std::string name = value;
          self->command(
              [name] { services::walletLifecycle("password", name); });
        },
        this);
    tabs.begin();
    trayTab = new Fl_Group(12, 66, 916, 538, "Tray applications");
    trayList = new Fl_Hold_Browser(28, 90, 330, 430);
    trayList->format_char(0);
    on(trayList, [this] { openTray(); });
    trayRows = new Fl_Hold_Browser(374, 90, 534, 430);
    trayRows->format_char(0);
    on(trayRows, [this] {
      if (Fl::event_clicks())
        trayAction();
    });
    button(28, 548, 140, "Refresh", [this] { refreshTray(); });
    button(374, 548, 140, "Run action", [this] { trayAction(); });
    button(530, 548, 140, "Back", [this] {
      if (parents.size() > 1)
        parents.pop_back();
      loadTrayMenu(parents.back());
    });
    trayTab->end();
    deviceTab = new Fl_Group(12, 66, 916, 538, "KDE Connect");
    deviceList = new Fl_Hold_Browser(28, 90, 330, 330);
    deviceList->format_char(0);
    on(deviceList, [this] { selectDevice(); });
    deviceInfo = new Fl_Multiline_Output(374, 90, 534, 170);
    plugins = new Fl_Hold_Browser(374, 278, 534, 142);
    plugins->format_char(0);
    button(28, 435, 100, "Discover", [this] { deviceAction("refresh"); });
    button(140, 435, 100, "Pair", [this] { deviceAction("pair"); });
    button(252, 435, 100, "Accept", [this] { deviceAction("accept"); });
    button(374, 435, 100, "Cancel pair", [this] { deviceAction("cancel"); });
    button(486, 435, 100, "Unpair", [this] {
      if (fl_choice("Remove pairing with this device?", "Cancel", "Unpair",
                    nullptr) == 1)
        deviceAction("unpair");
    });
    button(598, 435, 100, "Ping",
           [this] { deviceAction("ping", "Hello from DomeD"); });
    button(710, 435, 100, "Find phone", [this] { deviceAction("ring"); });
    button(28, 478, 140, "Share file…", [this] {
      Fl_Native_File_Chooser chooser;
      chooser.title("Share a file");
      if (!chooser.show() && chooser.filename()) {
        auto uri = g_filename_to_uri(chooser.filename(), nullptr, nullptr);
        if (uri) {
          deviceAction("url", uri);
          g_free(uri);
        }
      }
    });
    button(184, 478, 140, "Share URL…", [this] {
      auto value = fl_input("URL to share:");
      if (value)
        deviceAction("url", value);
    });
    button(340, 478, 140, "Share text…", [this] {
      auto value = fl_input("Text to share:");
      if (value)
        deviceAction("text", value);
    });
    button(496, 478, 140, "Enable plugin", [this] {
      auto p = selected(plugins);
      if (!p.empty())
        deviceAction("plugin-on", p);
    });
    button(652, 478, 140, "Disable plugin", [this] {
      auto p = selected(plugins);
      if (!p.empty())
        deviceAction("plugin-off", p);
    });
    button(28, 524, 180, "Rename this computer…", [this] {
      auto v = fl_input("Computer name:");
      if (v)
        deviceAction("rename", v);
    });
    deviceTab->end();
    inputTab = new Fl_Group(12, 66, 916, 538, "Input methods");
    groups = new Fl_Choice(110, 82, 230, 30, "Group:");
    on(groups, [this] { loadGroup(); });
    button(354, 82, 100, "Activate", [this] {
      auto g = group();
      command([g] { services::inputAction("group", g); });
    });
    button(466, 82, 100, "New…", [this] {
      auto v = fl_input("New input group:");
      if (v) {
        std::string n = v;
        command([n] { services::inputAction("add-group", n); },
                [this] { refreshInput(); });
      }
    });
    button(578, 82, 100, "Delete", [this] {
      auto g = group();
      if (fl_choice("Delete this input group?", "Cancel", "Delete", nullptr) ==
          1)
        command([g] { services::inputAction("remove-group", g); },
                [this] { refreshInput(); });
    });
    button(690, 82, 100, "Toggle IM", [this] {
      command([] { services::inputAction("toggle", ""); },
              [this] { refreshInput(); });
    });
    available =
        new Fl_Hold_Browser(28, 140, 410, 170, "Available input methods");
    available->align(FL_ALIGN_TOP_LEFT);
    available->format_char(0);
    enabled = new Fl_Hold_Browser(476, 140, 432, 170, "Methods in group");
    enabled->align(FL_ALIGN_TOP_LEFT);
    enabled->format_char(0);
    button(28, 322, 120, "Use now", [this] {
      int i = available->value() - 1;
      if (i < 0)
        return;
      auto id = inputData["methods"][i]["id"].get<std::string>();
      command([id] { services::inputAction("select", id); });
    });
    button(160, 322, 120, "Add to group", [this] {
      int i = available->value() - 1;
      if (i < 0 || groupData.is_null())
        return;
      auto id = inputData["methods"][i]["id"];
      for (auto &m : groupData["methods"])
        if (m["id"] == id)
          return;
      groupData["methods"].push_back({{"id", id}, {"layout", ""}});
      updateGroup();
    });
    button(476, 322, 100, "Remove", [this] {
      int i = enabled->value() - 1;
      if (i >= 0) {
        groupData["methods"].erase(i);
        updateGroup();
      }
    });
    button(588, 322, 100, "Move up", [this] {
      int i = enabled->value() - 1;
      if (i > 0) {
        std::swap(groupData["methods"][i], groupData["methods"][i - 1]);
        updateGroup();
        enabled->value(i);
      }
    });
    button(700, 322, 100, "Move down", [this] {
      int i = enabled->value() - 1;
      if (i >= 0 && size_t(i + 1) < groupData["methods"].size()) {
        std::swap(groupData["methods"][i], groupData["methods"][i + 1]);
        updateGroup();
        enabled->value(i + 2);
      }
    });
    layout = new Fl_Input(110, 364, 230, 30, "Layout:");
    button(354, 364, 140, "Save group", [this] { saveGroup(); });
    button(514, 364, 180, "Method layout…", [this] {
      int i = enabled->value() - 1;
      if (i < 0)
        return;
      auto v = fl_input(
          "Layout override for this input method (empty uses default):",
          groupData["methods"][i]["layout"].get<std::string>().c_str());
      if (v) {
        groupData["methods"][i]["layout"] = v;
        updateGroup();
      }
    });
    configTarget = new Fl_Choice(110, 410, 230, 30, "Settings:");
    button(354, 410, 140, "Load settings", [this] { configureInput(); });
    configList = new Fl_Hold_Browser(28, 450, 650, 130);
    configList->format_char(0);
    on(configList, [this] {
      if (Fl::event_clicks())
        editConfig();
    });
    button(700, 450, 180, "Edit selected…", [this] { editConfig(); });
    button(700, 492, 180, "Save settings", [this] {
      auto uri = configUri;
      auto values = configuration;
      if (!values.is_null())
        command([uri, values] { services::saveInputConfig(uri, values); });
    });
    inputTab->end();
    walletTab = new Fl_Group(12, 66, 916, 538, "Wallet manager");
    walletList = new Fl_Choice(110, 82, 300, 30, "Wallet:");
    button(430, 82, 100, "Open", [this] { openWallet(); });
    button(542, 82, 100, "Lock", [this] { walletAction("lock"); });
    button(654, 82, 140, "Refresh", [this] { refreshWallets(); });
    folderList = new Fl_Hold_Browser(28, 145, 270, 270, "Folders");
    folderList->align(FL_ALIGN_TOP_LEFT);
    folderList->format_char(0);
    on(folderList, [this] { loadFolder(); });
    entryList = new Fl_Hold_Browser(316, 145, 592, 270, "Entries");
    entryList->align(FL_ALIGN_TOP_LEFT);
    entryList->format_char(0);
    on(entryList, [this] { selectedEntry(); });
    button(28, 430, 125, "New folder…", [this] {
      auto f = fl_input("Folder name:");
      if (f) {
        selectedFolder = f;
        walletAction("create-folder");
      }
    });
    button(165, 430, 130, "Delete folder", [this] {
      if (fl_choice("Delete this folder and all of its entries?", "Cancel",
                    "Delete", nullptr) == 1)
        walletAction("remove-folder");
    });
    button(316, 430, 125, "Read password", [this] { readEntry(); });
    button(453, 430, 125, "Rename…", [this] {
      auto name = fl_input("New entry name:", entryName->value());
      if (name)
        walletAction("rename", entryName->value(), name);
    });
    button(590, 430, 125, "Delete entry", [this] {
      if (fl_choice("Delete this wallet entry?", "Cancel", "Delete", nullptr) ==
          1)
        walletAction("remove", entryName->value());
    });
    entryName = new Fl_Input(110, 480, 390, 30, "Entry:");
    password = new Fl_Secret_Input(110, 524, 390, 30, "Password:");
    button(524, 480, 180, "Write password", [this] {
      if (selectedFolder.empty() || !*entryName->value()) {
        message("Select a folder and enter an entry name.");
        return;
      }
      if (fl_choice("Save this password in the selected wallet entry?",
                    "Cancel", "Save", nullptr) == 1)
        walletAction("write", entryName->value(), password->value());
    });
    button(524, 524, 180, "Clear password", [this] { clearSecret(); });
    button(724, 480, 180, "Edit map / binary…", [this] { dataEditor(true); });
    button(724, 524, 180, "New map / binary…", [this] { dataEditor(false); });
    walletTab->end();
    tabs.end();
    on(&tabs, [this] {
      clearSecret();
      refresh();
    });
    status.align(FL_ALIGN_LEFT | FL_ALIGN_INSIDE | FL_ALIGN_WRAP);
    end();
    xclass("org.domed.Tray");
    size_range(940, 660);
    dx::style(this);
    callback([](Fl_Widget *, void *p) { ((Manager *)p)->close(); }, this);
    Fl::add_timeout(.04, tick, this);
  }
  ~Manager() {
    Fl::remove_timeout(tick, this);
    clearSecret();
  }
  void close() {
    clearSecret();
    hide();
  }
  void showPage(const std::string &name) {
    requestedInputConfig.clear();
    const bool input = name == "input" || name.rfind("input:fcitx://config/", 0) == 0;
    if (name.rfind("input:fcitx://config/", 0) == 0)
      requestedInputConfig = name.substr(6);
    tabs.value(name == "devices"   ? deviceTab
               : input             ? inputTab
               : name == "wallets" ? walletTab
                                   : trayTab);
    show();
    refresh();
  }
  void refresh() {
    if (tabs.value() == deviceTab)
      refreshDevices();
    else if (tabs.value() == inputTab)
      refreshInput();
    else if (tabs.value() == walletTab)
      refreshWallets();
    else
      refreshTray();
  }
  bool running() { return !quitting; }
};
int main(int argc, char **argv) try {
#ifdef DOME_FCITX_CONFIGURATION
  bool showWindow = true;
  std::string page = "input";
#else
  bool showWindow = false;
  std::string page = "tray";
#endif
  for (int i = 1; i < argc; ++i) {
    std::string a = argv[i];
    if (a == "--daemon")
      showWindow = false;
    else if (a == "--tray" || a == "--devices" || a == "--input" ||
             a == "--wallets") {
      page = a.substr(2);
      showWindow = true;
    } else if (a == "--help") {
#ifdef DOME_FCITX_CONFIGURATION
      puts("fcitx5-configuration-fltk [fcitx://config/global|fcitx://config/inputmethod/ID|fcitx://config/addon/ID]");
#else
      puts("dome-tray [--daemon|--tray|--devices|--input|--wallets]\n"
           "Close hides the manager; File > Quit exits the tray companion.");
#endif
      return 0;
    } else if (a.rfind("fcitx://config/", 0) == 0) {
      page = "input:" + a;
      showWindow = true;
    } else
      throw std::runtime_error("Unknown option: " + a);
  }
  if (Notifier::showExisting(showWindow ? page : ""))
    return 0;
  struct rlimit limit = {0, 0};
  setrlimit(RLIMIT_CORE, &limit);
  dx::init();
  Manager window;
  Notifier notifier([&window](const std::string &p) { window.showPage(p); });
  if (showWindow)
    window.showPage(page);
  while (window.running())
    Fl::wait(.1);
  return 0;
} catch (const std::exception &e) {
  fprintf(stderr, "dome-tray: %s\n", e.what());
  return 1;
}
