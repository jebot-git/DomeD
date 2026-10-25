// SPDX-License-Identifier: MIT
#include "desktop-menu.h"
#include "gpu-window.h"
#include "notifier.h"
#include "services.h"
#include "bluetooth.h"
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
  Fl_Group *trayTab, *deviceTab, *inputTab, *walletTab, *bluetoothTab;
  bluetooth::Client bluetoothClient;
  Fl_Choice *bluetoothAdapters;
  Fl_Hold_Browser *bluetoothDevices;
  Fl_Multiline_Output *bluetoothInfo, *bluetoothPrompt;
  Fl_Box *bluetoothStatus;
  Fl_Secret_Input *bluetoothCode;
  Fl_Button *bluetoothPower, *bluetoothScan, *bluetoothVisible,
      *bluetoothPairable, *bluetoothAccept, *bluetoothReject;
  std::map<std::string, Fl_Button *> bluetoothButtons;
  Json bluetoothState, bluetoothRows = Json::array();
  std::string bluetoothAdapter, bluetoothDevice, bluetoothRevision;
  unsigned bluetoothPromptId = 0;
  Fl_Box status{12, 612, 916, 36};
  Fl_Hold_Browser *trayList, *trayRows, *deviceList, *plugins, *available,
      *enabled, *folderList, *entryList, *configList;
  Fl_Multiline_Output *deviceInfo;
  Fl_Button *moveToManager, *moveToTray;
  Fl_Choice *groups, *configTarget;
  Fl_Input *layout, *entryName;
  Fl_Secret_Input *password;
  Json trayData = Json::array(), rows = Json::array(),
       deviceData = Json::array(), inputData, groupData,
       entryData = Json::array(), configuration;
  Json selectedTray;
  std::vector<int> parents{0};
  std::string selectedDevice, walletName, selectedFolder, loadedEntry,
      configUri, requestedInputConfig, requestedSuite;
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
  int walletHandle = -1;
  Fl_Choice *walletList;
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
    self->updateBluetooth();
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
      const auto selected = requestedSuite.empty() && selectedTray.is_object()
          ? selectedTray.value("Id", std::string()) : requestedSuite;
      requestedSuite.clear();
      trayData = std::move(data);
      trayList->clear();
      for (auto &item : trayData)
        trayList->add((item.value("Title", item.value("Id", std::string("Application"))) +
            (item.value("movable", false) ? (item.value("managed", false) ? " [manager]" : " [tray]") : "")).c_str());
      for (size_t i = 0; i < trayData.size(); ++i)
        if (trayData[i].value("Id", std::string()) == selected) {
          trayList->value(i + 1);
          openTray();
          break;
        }
    });
  }
  void openTray() {
    int index = trayList->value() - 1;
    if (index < 0 || size_t(index) >= trayData.size())
      return;
    selectedTray = trayData[index];
    moveToManager->deactivate(); moveToTray->deactivate();
    if (selectedTray.value("movable", false))
      (selectedTray.value("managed", false) ? moveToTray : moveToManager)->activate();
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
    if (row.value("id", 0) == -1) {
      showPage(services::frontend(selectedTray.value("Id", std::string()) + " " +
                                 selectedTray.value("Title", std::string())));
      return;
    }
    if (row.value("submenu", false)) {
      parents.push_back(row.at("id"));
      loadTrayMenu(parents.back());
    } else {
      auto item = selectedTray;
      if (item.value("suite", "") == "dome.screenshot") hide();
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
          walletHandle = d.at("handle");
          folderList->clear();
          entryList->clear();
          for (auto &f : d["folders"])
            folderList->add(f.get<std::string>().c_str());
        });
  }
  void loadFolder() {
    if (walletHandle < 0)
      return;
    selectedFolder = selected(folderList);
    auto h = walletHandle;
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
    if (walletHandle < 0) {
      message("Open a wallet first.");
      return;
    }
    auto h = walletHandle;
    auto f = selectedFolder;
    command([action, h, f, key,
             value] { services::walletAction(action, h, f, key, value); },
            [this, action] {
              clearSecret();
              if (action == "lock") {
                walletHandle = -1;
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
    if (walletHandle < 0)
      return;
    int i = entryList->value() - 1;
    if (i < 0 || size_t(i) >= entryData.size())
      return;
    if (entryData[i]["type"] != 1) {
      message(
          "This entry contains a map or binary stream; use the data editor.");
      return;
    }
    auto h = walletHandle;
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
    if (walletHandle < 0 || selectedFolder.empty()) {
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
        auto h = walletHandle;
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
      auto h = walletHandle;
      auto folder = selectedFolder;
      submit([h, folder, key] { return services::readData(h, folder, key); },
             edit);
    } else
      edit({{"type", 3}, {"data", "{}"}});
  }

  Json bluetoothSelection() const {
    for (const auto &row : bluetoothRows)
      if (row["path"] == bluetoothDevice)
        return row;
    return Json::object();
  }
  void bluetoothAction(const std::string &op, bool adapter = false) {
    bluetoothClient.action(adapter ? bluetoothAdapter : bluetoothDevice, op);
  }
  void updateBluetooth(bool force = false) {
    auto data = bluetoothClient.snapshot();
    auto revision = data.dump();
    if (!force && revision == bluetoothRevision)
      return;
    bluetoothRevision = revision;
    bluetoothState = data;
    auto &adapters = data["adapters"];
    bluetoothAdapters->clear();
    int selectedAdapter = -1;
    for (size_t i = 0; i < adapters.size(); ++i) {
      auto label = adapters[i].value("Alias", std::string("Bluetooth")) + " (" +
                   adapters[i].value("Address", std::string()) + ")";
      // FLTK menu labels use slashes as submenu separators.
      size_t at = 0;
      while ((at = label.find('/', at)) != std::string::npos) {
        label.replace(at, 1, "\\/");
        at += 2;
      }
      bluetoothAdapters->add(label.c_str());
      if (adapters[i]["path"] == bluetoothAdapter)
        selectedAdapter = i;
    }
    if (selectedAdapter < 0 && !adapters.empty())
      selectedAdapter = 0;
    bluetoothAdapters->value(selectedAdapter);
    Json adapter =
        selectedAdapter < 0 ? Json::object() : adapters[selectedAdapter];
    bluetoothAdapter = adapter.value("path", std::string());
    bluetoothRows = Json::array();
    bluetoothDevices->clear();
    int selectedRow = 0;
    for (const auto &d : data["devices"])
      if (d.value("Adapter", std::string()) == bluetoothAdapter) {
        bluetoothRows.push_back(d);
        auto label =
            d.value("Alias", d.value("Address", std::string("Device"))) +
            (d.value("Connected", false) ? " — connected"
             : d.value("Paired", false)  ? " — paired"
                                         : " — nearby");
        bluetoothDevices->add(label.c_str());
        if (d["path"] == bluetoothDevice)
          selectedRow = bluetoothRows.size();
      }
    bluetoothDevices->value(selectedRow);
    if (!selectedRow)
      bluetoothDevice.clear();
    auto d = bluetoothSelection();
    std::string details;
    if (!d.empty()) {
      details = d.value("Alias", std::string()) + "\n" +
                d.value("Address", std::string()) + "\n" +
                (d.value("Connected", false) ? "Connected" : "Disconnected") +
                " · " + (d.value("Paired", false) ? "Paired" : "Not paired") +
                " · " +
                (d.value("Trusted", false) ? "Trusted" : "Not trusted") +
                (d.value("Blocked", false) ? " · Blocked" : "");
      if (d.contains("battery"))
        details +=
            "\nBattery: " + std::to_string(d["battery"].get<int>()) + "%";
      if (d.contains("RSSI"))
        details +=
            " · Signal: " + std::to_string(d["RSSI"].get<int>()) + " dBm";
    }
    bluetoothInfo->value(details.empty() ? "Select a device to see its details."
                                         : details.c_str());
    bool powered = adapter.value("Powered", false),
         scanning = adapter.value("scanning", false);
    bluetoothPower->copy_label(powered ? "Power off" : "Power on");
    bluetoothScan->copy_label(scanning ? "Stop scan" : "Scan (60 sec)");
    bluetoothVisible->copy_label(adapter.value("Discoverable", false)
                                     ? "Hide adapter"
                                     : "Visible (3 min)");
    bluetoothPairable->copy_label(
        adapter.value("Pairable", false) ? "Disable pairing" : "Allow pairing");
    auto isBusy = [&](const std::string &path) {
      return std::find(data["busy"].begin(), data["busy"].end(), path) !=
             data["busy"].end();
    };
    auto enable = [](Fl_Widget *w, bool on) {
      if (on)
        w->activate();
      else
        w->deactivate();
    };
    enable(bluetoothAdapters, !adapters.empty());
    enable(bluetoothPower, !adapter.empty() && !isBusy(bluetoothAdapter));
    enable(bluetoothScan, powered && !isBusy(bluetoothAdapter));
    enable(bluetoothVisible, powered && !isBusy(bluetoothAdapter));
    enable(bluetoothPairable, powered && !isBusy(bluetoothAdapter));
    for (auto &[op, b] : bluetoothButtons)
      enable(b, powered && !d.empty() && !isBusy(bluetoothDevice));
    enable(bluetoothButtons["cancel"],
           !d.empty() &&
               std::find(data["pairing"].begin(), data["pairing"].end(),
                         bluetoothDevice) != data["pairing"].end());
    enable(bluetoothButtons["pair"],
           powered && !d.empty() && !d.value("Paired", false) &&
               !d.value("Blocked", false) && data.value("agent", false) &&
               !isBusy(bluetoothDevice));
    enable(bluetoothButtons["connect"],
           powered && !d.empty() && !d.value("Connected", false) &&
               !d.value("Blocked", false) && !isBusy(bluetoothDevice));
    enable(bluetoothButtons["disconnect"], !d.empty() &&
                                               d.value("Connected", false) &&
                                               !isBusy(bluetoothDevice));
    bluetoothButtons["trust"]->copy_label(d.value("Trusted", false) ? "Untrust"
                                                                    : "Trust");
    bluetoothButtons["block"]->copy_label(d.value("Blocked", false) ? "Unblock"
                                                                    : "Block");
    auto status = data.value("error", std::string());
    if (status.empty())
      status = adapters.empty()
                   ? "No Bluetooth adapters found. Check the adapter "
                     "connection and hardware radio switch."
               : !powered ? "Bluetooth is powered off. If power-on fails, "
                            "check airplane mode or the hardware radio switch."
               : scanning ? "Scanning for nearby devices for up to 60 seconds…"
               : !data["busy"].empty() ? "Bluetooth operation in progress…"
                                       : "Ready. Scan to find nearby devices.";
    bluetoothStatus->copy_label(status.c_str());
    auto prompt = data["prompt"];
    if (prompt.is_null()) {
      bluetoothPromptId = 0;
      bluetoothPrompt->value("");
      bluetoothCode->value("");
      bluetoothCode->hide();
      bluetoothAccept->hide();
      bluetoothReject->hide();
    } else {
      auto id = prompt["id"].get<unsigned>();
      auto kind = prompt.value("kind", std::string());
      bool input = kind == "RequestPinCode" || kind == "RequestPasskey";
      bool display = kind == "DisplayPinCode" || kind == "DisplayPasskey";
      std::string identity = prompt["device"];
      for (const auto &device : data["devices"])
        if (device["path"] == prompt["device"])
          identity = device.value("Alias", std::string()) + " (" +
                     device.value("Address", std::string()) + ")";
      std::string text = identity + "\n";
      if (input)
        text += kind == "RequestPinCode"
                    ? "Enter the device PIN (1–16 bytes)."
                    : "Enter the six-digit device passkey.";
      else if (display)
        text += "Type " + prompt.value("code", std::string()) +
                " on the remote device" +
                (kind == "DisplayPasskey"
                     ? " (" + std::to_string(prompt.value("entered", 0)) +
                           " digits entered)"
                     : "") +
                ".";
      else if (kind == "RequestConfirmation")
        text += "Confirm that both devices show " +
                prompt.value("code", std::string()) + ".";
      else
        text += "Allow " +
                std::string(kind == "AuthorizeService"
                                ? "Bluetooth service " +
                                      prompt.value("uuid", std::string())
                                : "pairing with this device") +
                "?";
      bluetoothPrompt->value(text.c_str());
      bluetoothAccept->copy_label(display ? "Dismiss"
                                  : input ? "Submit"
                                          : "Confirm");
      bluetoothAccept->show();
      if (display)
        bluetoothReject->hide();
      else
        bluetoothReject->show();
      if (input)
        bluetoothCode->show();
      else
        bluetoothCode->hide();
      if (id != bluetoothPromptId) {
        bluetoothPromptId = id;
        bluetoothCode->value("");
        if (tabs.value() != bluetoothTab || !visible())
          showPage("bluetooth");
        if (input)
          bluetoothCode->take_focus();
        else
          bluetoothReject->take_focus();
      }
    }
    redraw();
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
    menu.add("&View/&Bluetooth", FL_ALT + '5',
             [](Fl_Widget *, void *p) { ((Manager *)p)->showPage("bluetooth"); }, this);
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
                            self->walletHandle = -1;
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
    moveToManager = button(28, 508, 158, "Move to manager", [this] {
      auto item = selectedTray;
      command([item] { services::trayPlacement(item, true); }, [this] { refreshTray(); });
    });
    moveToTray = button(192, 508, 158, "Move to tray", [this] {
      auto item = selectedTray;
      command([item] { services::trayPlacement(item, false); }, [this] { refreshTray(); });
    });
    moveToManager->deactivate(); moveToTray->deactivate();
    trayList->size(330, 398);
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
    bluetoothTab = new Fl_Group(12, 66, 916, 538, "Bluetooth");
    bluetoothAdapters = new Fl_Choice(110, 88, 420, 30, "Adapter:");
    on(bluetoothAdapters, [this] {
      bluetoothClient.stopDiscovery();
      int i = bluetoothAdapters->value();
      if (i >= 0 && size_t(i) < bluetoothState["adapters"].size())
        bluetoothAdapter = bluetoothState["adapters"][i]["path"];
      bluetoothDevice.clear();
      updateBluetooth(true);
    });
    bluetoothPower = button(550, 88, 150, "Power on", [this] {
      auto &a = bluetoothState["adapters"];
      for (const auto &row : a)
        if (row["path"] == bluetoothAdapter)
          bluetoothAction(
              row.value("Powered", false) ? "power-off" : "power-on", true);
    });
    bluetoothScan = button(716, 88, 192, "Scan (60 sec)", [this] {
      for (const auto &row : bluetoothState["adapters"])
        if (row["path"] == bluetoothAdapter)
          bluetoothAction(row.value("scanning", false) ? "stop" : "scan", true);
    });
    bluetoothVisible = button(28, 134, 190, "Visible (3 min)", [this] {
      for (const auto &row : bluetoothState["adapters"])
        if (row["path"] == bluetoothAdapter)
          bluetoothAction(row.value("Discoverable", false) ? "visible-off"
                                                           : "visible-on",
                          true);
    });
    bluetoothPairable = button(234, 134, 190, "Allow pairing", [this] {
      for (const auto &row : bluetoothState["adapters"])
        if (row["path"] == bluetoothAdapter)
          bluetoothAction(row.value("Pairable", false) ? "pairable-off"
                                                       : "pairable-on",
                          true);
    });
    button(440, 134, 150, "Refresh", [this] { bluetoothClient.refresh(); });
    bluetoothDevices = new Fl_Hold_Browser(28, 198, 350, 246, "Devices");
    bluetoothDevices->align(FL_ALIGN_TOP_LEFT);
    bluetoothDevices->format_char(0);
    on(bluetoothDevices, [this] {
      int i = bluetoothDevices->value() - 1;
      bluetoothDevice = i >= 0 && size_t(i) < bluetoothRows.size()
                            ? bluetoothRows[i]["path"].get<std::string>()
                            : "";
      updateBluetooth(true);
    });
    bluetoothInfo = new Fl_Multiline_Output(394, 198, 514, 104);
    int n = 0;
    for (const auto &item : std::vector<std::pair<std::string, const char *>>{
             {"pair", "Pair"},
             {"cancel", "Cancel pair"},
             {"connect", "Connect"},
             {"disconnect", "Disconnect"},
             {"trust", "Trust"},
             {"block", "Block"},
             {"remove", "Remove…"}}) {
      auto op = item.first;
      bluetoothButtons[op] = button(
          394 + (n % 3) * 174, 320 + (n / 3) * 44, 164, item.second,
          [this, op] {
            auto action = op;
            auto d = bluetoothSelection();
            if (op == "trust" && d.value("Trusted", false))
              action = "untrust";
            if (op == "block" && d.value("Blocked", false))
              action = "unblock";
            if (op == "remove" &&
                fl_choice("Remove this Bluetooth device and its saved pairing?",
                          "Cancel", "Remove", nullptr) != 1)
              return;
            bluetoothAction(action);
          });
      ++n;
    }
    bluetoothPrompt = new Fl_Multiline_Output(28, 460, 880, 46);
    bluetoothCode = new Fl_Secret_Input(28, 516, 300, 30);
    bluetoothAccept = button(344, 516, 150, "Confirm", [this] {
      bluetoothClient.respond(bluetoothPromptId, true, bluetoothCode->value());
      bluetoothCode->value("");
    });
    bluetoothReject = button(510, 516, 150, "Reject", [this] {
      bluetoothClient.respond(bluetoothPromptId, false);
      bluetoothCode->value("");
    });
    bluetoothStatus = new Fl_Box(28, 552, 880, 44);
    bluetoothStatus->align(FL_ALIGN_LEFT | FL_ALIGN_INSIDE | FL_ALIGN_WRAP);
    bluetoothTab->end();
    tabs.end();
    updateBluetooth(true);
    on(&tabs, [this] {
      if(tabs.value()!=bluetoothTab)
        bluetoothClient.stopDiscovery();
      clearSecret();
      refresh();
    });
    status.align(FL_ALIGN_LEFT | FL_ALIGN_INSIDE | FL_ALIGN_WRAP);
    end();
    xclass("org.domed.Tray");
    if (dx::dome()) {
      // Keep the command model for DBusMenu; DomeD draws the only menu bar.
      menu.hide();
      menu.global(); // Hidden menu widgets still own the application shortcuts.
      tabs.position(tabs.x(), tabs.y() - menu.h());
      status.position(status.x(), status.y() - menu.h());
      size(w(), h() - menu.h());
    }
    size_range(w(), h());
    dx::style(this);
    callback([](Fl_Widget *, void *p) { ((Manager *)p)->close(); }, this);
    Fl::add_timeout(.04, tick, this);
  }
  int handle(int event) override {
    if ((event == FL_KEYDOWN || event == FL_SHORTCUT) &&
        Fl::event_key() == FL_Escape && !Fl::modal()) {
      close();
      return 1;
    }
    return dx::GpuWindow::handle(event);
  }
  ~Manager() {
    Fl::remove_timeout(tick, this);
    clearSecret();
  }
  void close() {
    bluetoothClient.stopDiscovery();
    bluetoothClient.cancelPairing();
    bluetoothCode->value("");
    clearSecret();
    hide();
  }
  void showPage(const std::string &name) {
    if(name!="bluetooth")bluetoothClient.stopDiscovery();
    requestedInputConfig.clear();
    requestedSuite = name == "translator" ? "dome.translator"
                   : name == "screenshot" ? "dome.screenshot" : "";
    const bool input = name == "input" || name.rfind("input:fcitx://config/", 0) == 0;
    if (name.rfind("input:fcitx://config/", 0) == 0)
      requestedInputConfig = name.substr(6);
    tabs.value(name == "bluetooth" ? bluetoothTab
               : name == "devices"   ? deviceTab
               : input             ? inputTab
               : name == "wallets" ? walletTab
                                   : trayTab);
    show();
    refresh();
  }
  void refresh() {
    if (tabs.value() == bluetoothTab) { bluetoothClient.refresh(); updateBluetooth(true); }
    else if (tabs.value() == deviceTab)
      refreshDevices();
    else if (tabs.value() == inputTab)
      refreshInput();
    else if (tabs.value() == walletTab)
      refreshWallets();
    else
      refreshTray();
  }
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
             a == "--bluetooth" || a == "--wallets" || a == "--translator" || a == "--screenshot") {
      page = a.substr(2);
      showWindow = true;
    } else if (a == "--help") {
#ifdef DOME_FCITX_CONFIGURATION
      puts("fcitx5-configuration-fltk [fcitx://config/global|fcitx://config/inputmethod/ID|fcitx://config/addon/ID]");
#else
      puts("dome-tray [--daemon|--tray|--devices|--input|--wallets|--bluetooth|--translator|--screenshot]\n"
           "Close hides the permanent session tray manager.");
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
  while (true)
    Fl::wait(.1);
  return 0;
} catch (const std::exception &e) {
  fprintf(stderr, "dome-tray: %s\n", e.what());
  return 1;
}
