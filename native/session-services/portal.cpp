// SPDX-License-Identifier: MIT
// Native dialogs behind the standard xdg-desktop-portal frontend. The frontend
// retains sandbox checks, permission storage and document-FUSE registration.
#include "gpu-window.h"
#include "login-session.h"
#include <FL/Fl_Box.H>
#include <FL/Fl_Button.H>
#include <FL/Fl_Check_Button.H>
#include <FL/Fl_Choice.H>
#include <FL/Fl_Input.H>
#include <FL/Fl_Int_Input.H>
#include <FL/Fl_Multi_Browser.H>
#include <FL/Fl_PNG_Image.H>
#include <FL/Fl_Return_Button.H>
#include <FL/Fl_Scroll.H>
#include <filesystem>
#include <fnmatch.h>
#include <functional>
#include <gio/gdesktopappinfo.h>
#include <gio/gio.h>
#include <gio/gunixfdlist.h>
#include <map>
#include <pwd.h>
#include <set>
#include <signal.h>
#include <sstream>
#include <sys/resource.h>
#include <unistd.h>

namespace fs = std::filesystem;
static GDBusConnection *bus;
static std::map<std::string, GDBusNodeInfo *> xml;
static std::string frontend;
static bool running = true;
static std::string text(GVariant *v, const char *key,
                        const char *fallback = "") {
  const char *s;
  return v && g_variant_lookup(v, key, "&s", &s) ? s : fallback;
}
static bool boolean(GVariant *v, const char *key, bool fallback = false) {
  gboolean value;
  return v && g_variant_lookup(v, key, "b", &value) ? bool(value) : fallback;
}
static std::string string_at(GVariant *v, size_t n) {
  auto item = g_variant_get_child_value(v, n);
  auto value = std::string(g_variant_get_string(item, nullptr));
  g_variant_unref(item);
  return value;
}
static std::string menu_label(const std::string &s) {
  std::string out;
  for (char c : s) {
    if (strchr("/\\&_|", c))
      out += '\\';
    out += c;
  }
  return out;
}
class IconPreview : public Fl_Box {
  std::vector<uchar> pixels;
  int width = 0, height = 0, depth = 0;

public:
  IconPreview(int x, int y, GVariant *serialized) : Fl_Box(x, y, 96, 96) {
    auto value = g_variant_get_variant(serialized);
    auto icon = g_icon_deserialize(value);
    g_variant_unref(value);
    if (!icon)
      return;
    if (G_IS_BYTES_ICON(icon)) {
      auto bytes = g_bytes_icon_get_bytes(G_BYTES_ICON(icon));
      gsize size = 0;
      auto data = static_cast<const uchar *>(g_bytes_get_data(bytes, &size));
      std::unique_ptr<Fl_RGB_Image> image;
      if (size >= 24 && size <= 1024 * 1024 &&
          !memcmp(data, "\211PNG\r\n\032\n", 8)) {
        auto read = [data](int at) {
          return (guint(data[at]) << 24) | (guint(data[at + 1]) << 16) |
                 (guint(data[at + 2]) << 8) | guint(data[at + 3]);
        };
        if (read(16) <= 1024 && read(20) <= 1024)
          image = std::make_unique<Fl_PNG_Image>("launcher-icon", data, size);
      } else if (size && size <= 512 * 1024) {
        std::string svg(reinterpret_cast<const char *>(data), size);
        auto vector = std::make_unique<Fl_SVG_Image>(nullptr, svg.c_str());
        if (!vector->fail()) {
          vector->resize(96, 96);
          vector->normalize();
          image = std::move(vector);
        }
      }
      if (image && !image->fail()) {
        std::unique_ptr<Fl_RGB_Image> scaled(
            static_cast<Fl_RGB_Image *>(image->copy(96, 96)));
        width = scaled->w();
        height = scaled->h();
        depth = scaled->d();
        auto stride = scaled->ld() ? scaled->ld() : width * depth;
        for (int row = 0; row < height; ++row)
          pixels.insert(pixels.end(), scaled->array + row * stride,
                        scaled->array + row * stride + width * depth);
      }
    }
    g_object_unref(icon);
  }
  bool valid() const { return !pixels.empty(); }
  void draw() override {
    Fl_Box::draw();
    if (pixels.empty())
      return;
    auto window = dynamic_cast<Fl_Gl_Window *>(Fl_Window::current());
    float scale = window ? window->pixels_per_unit() : 1.f;
    glPushAttrib(GL_COLOR_BUFFER_BIT | GL_PIXEL_MODE_BIT | GL_CURRENT_BIT |
                 GL_ENABLE_BIT);
    glEnable(GL_BLEND);
    glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
    glPixelZoom(scale, -scale);
    gl_draw_image(pixels.data(), x(), y(), width, height, depth, width * depth);
    glPopAttrib();
  }
};
static std::string bytes(GVariant *v) {
  if (!v)
    return {};
  gsize length;
  auto data =
      static_cast<const char *>(g_variant_get_fixed_array(v, &length, 1));
  if (!length || data[length - 1] != 0)
    return {};
  return std::string(data, length - 1);
}
static GVariant *empty_dict() {
  GVariantBuilder b;
  g_variant_builder_init(&b, G_VARIANT_TYPE_VARDICT);
  return g_variant_builder_end(&b);
}
static void fail(GDBusMethodInvocation *reply, const char *message) {
  g_dbus_method_invocation_return_dbus_error(
      reply, "org.freedesktop.portal.Error.Failed", message);
}
static bool trusted(const char *sender, GDBusMethodInvocation *reply) {
  auto result = g_dbus_connection_call_sync(
      bus, "org.freedesktop.DBus", "/org/freedesktop/DBus",
      "org.freedesktop.DBus", "GetNameOwner",
      g_variant_new("(s)", "org.freedesktop.portal.Desktop"),
      G_VARIANT_TYPE("(s)"), G_DBUS_CALL_FLAGS_NONE, 2000, nullptr, nullptr);
  bool ok = result && string_at(result, 0) == sender;
  if (result) {
    frontend = string_at(result, 0);
    g_variant_unref(result);
  }
  if (!ok)
    g_dbus_method_invocation_return_dbus_error(
        reply, "org.freedesktop.DBus.Error.AccessDenied",
        "Only xdg-desktop-portal may call this backend");
  return ok;
}
struct Request;
static std::map<std::string, std::shared_ptr<Request>> requests;
struct Choice {
  std::string id;
  std::vector<std::string> values;
  Fl_Choice *menu = nullptr;
  Fl_Check_Button *check = nullptr;
};
struct Request : std::enable_shared_from_this<Request> {
  std::string handle, app, method;
  GDBusMethodInvocation *reply;
  GVariant *options;
  dx::GpuWindow *window;
  Fl_Box *message, *status;
  Fl_Button *cancel;
  Fl_Return_Button *accept;
  guint registration = 0;
  std::function<void()> action;
  std::vector<Choice> choices;
  Fl_Multi_Browser *list = nullptr;
  Fl_Input *directory = nullptr, *filename = nullptr;
  Fl_Choice *filters = nullptr;
  std::vector<GVariant *> filter_data;
  std::vector<fs::path> paths;
  std::vector<std::string> applications;
  bool directories = false, multiple = false, done = false,
       overwrite_confirmed = false;
  int fd = -1;
  GSubprocess *process = nullptr;
  Request(std::string h, std::string a, std::string m, std::string title,
          GVariant *o, GDBusMethodInvocation *r)
      : handle(std::move(h)), app(std::move(a)), method(std::move(m)),
        reply(G_DBUS_METHOD_INVOCATION(g_object_ref(r))),
        options(g_variant_ref(o)) {
    window = new dx::GpuWindow(700, 650, "DomeD Application Request");
    message = new Fl_Box(20, 12, 660, 65);
    message->copy_label(
        (title + "\n" + (app.empty() ? "An application" : app)).c_str());
    message->align(FL_ALIGN_INSIDE | FL_ALIGN_LEFT | FL_ALIGN_WRAP);
    status = new Fl_Box(20, 541, 660, 40);
    status->align(FL_ALIGN_INSIDE | FL_ALIGN_LEFT | FL_ALIGN_WRAP);
    cancel = new Fl_Button(450, 600, 100, 32, "Cancel");
    accept = new Fl_Return_Button(560, 600, 120, 32, "Continue");
    window->end();
    window->set_modal();
    for (Fl_Widget *w :
         {static_cast<Fl_Widget *>(cancel), static_cast<Fl_Widget *>(window)})
      w->callback(
          [](Fl_Widget *, void *p) { static_cast<Request *>(p)->finish(1); },
          this);
    accept->callback(
        [](Fl_Widget *, void *p) {
          auto r = static_cast<Request *>(p)->shared_from_this();
          if (r->action)
            r->action();
        },
        this);
  }
  ~Request() {
    g_object_unref(reply);
    g_variant_unref(options);
    for (auto value : filter_data)
      g_variant_unref(value);
    if (fd >= 0)
      close(fd);
    if (process) {
      g_subprocess_force_exit(process);
      g_object_unref(process);
    }
  }
  void finish(guint response, GVariant *results = nullptr) {
    if (done)
      return;
    auto keep = shared_from_this();
    done = true;
    if (response && process)
      g_subprocess_force_exit(process);
    if (registration)
      g_dbus_connection_unregister_object(bus, registration);
    registration = 0;
    if (window) {
      window->hide();
      Fl::delete_widget(window);
      window = nullptr;
    }
    g_dbus_method_invocation_return_value(
        reply,
        g_variant_new("(u@a{sv})", response, results ? results : empty_dict()));
    requests.erase(handle);
  }
  void show() {
    window->end();
    dx::style(window);
    window->show();
  }
  void error(const std::string &s) {
    status->copy_label(s.c_str());
    window->redraw();
  }
  void add_choices(int y = 420) {
    auto values = g_variant_lookup_value(options, "choices",
                                         G_VARIANT_TYPE("a(ssa(ss)s)"));
    if (!values)
      return;
    window->begin();
    auto scroll = new Fl_Scroll(15, y, 675, 112);
    scroll->begin();
    for (gsize n = 0; n < g_variant_n_children(values); ++n) {
      auto item = g_variant_get_child_value(values, n);
      auto entries = g_variant_get_child_value(item, 2);
      Choice choice;
      choice.id = string_at(item, 0);
      auto label = string_at(item, 1);
      auto current = string_at(item, 3);
      if (g_variant_n_children(entries) == 0) {
        choice.check = new Fl_Check_Button(20, y + 28 * n, 660, 26);
        choice.check->copy_label(label.c_str());
        choice.check->value(current == "true");
      } else {
        choice.menu = new Fl_Choice(230, y + 28 * n, 450, 26);
        choice.menu->copy_label(label.c_str());
        int selected = 0;
        for (gsize j = 0; j < g_variant_n_children(entries); ++j) {
          auto entry = g_variant_get_child_value(entries, j);
          auto id = string_at(entry, 0);
          choice.values.push_back(id);
          choice.menu->add(menu_label(string_at(entry, 1)).c_str());
          if (id == current)
            selected = j;
          g_variant_unref(entry);
        }
        choice.menu->value(selected);
      }
      choices.push_back(std::move(choice));
      g_variant_unref(entries);
      g_variant_unref(item);
    }
    g_variant_unref(values);
    scroll->end();
    window->end();
  }
  void results_choices(GVariantBuilder &builder) {
    GVariantBuilder selected;
    g_variant_builder_init(&selected, G_VARIANT_TYPE("a(ss)"));
    for (auto &c : choices) {
      auto value = c.check ? std::string(c.check->value() ? "true" : "false")
                           : c.values.at(c.menu->value());
      g_variant_builder_add(&selected, "(ss)", c.id.c_str(), value.c_str());
    }
    g_variant_builder_add(&builder, "{sv}", "choices",
                          g_variant_builder_end(&selected));
  }
  bool matches(const fs::path &path) {
    if (!filters || filter_data.empty())
      return true;
    auto filter = filter_data.at(filters->value());
    auto patterns = g_variant_get_child_value(filter, 1);
    bool match = false;
    for (gsize i = 0; i < g_variant_n_children(patterns); ++i) {
      auto item = g_variant_get_child_value(patterns, i);
      guint type;
      const char *pattern;
      g_variant_get(item, "(u&s)", &type, &pattern);
      if (type == 0)
        match |= fnmatch(pattern, path.filename().c_str(), 0) == 0;
      else if (type == 1) {
        gboolean uncertain;
        auto content =
            g_content_type_guess(path.c_str(), nullptr, 0, &uncertain);
        auto wanted = g_content_type_from_mime_type(pattern);
        if (content && wanted)
          match |= g_content_type_is_a(content, wanted);
        g_free(content);
        g_free(wanted);
      }
      g_variant_unref(item);
    }
    bool no_patterns = g_variant_n_children(patterns) == 0;
    g_variant_unref(patterns);
    return match || no_patterns;
  }
  void load() {
    std::error_code e;
    auto path = fs::absolute(directory->value(), e).lexically_normal();
    if (e || !fs::is_directory(path, e)) {
      error("Choose an existing folder.");
      return;
    }
    directory->value(path.c_str());
    list->clear();
    paths.clear();
    overwrite_confirmed = false;
    std::vector<fs::path> entries;
    for (fs::directory_iterator
             i(path, fs::directory_options::skip_permission_denied, e),
         end;
         i != end && !e; i.increment(e)) {
      bool is_dir = i->is_directory(e);
      if (e) {
        e.clear();
        continue;
      }
      if (is_dir || (!directories && matches(i->path())))
        entries.push_back(i->path());
    }
    std::sort(entries.begin(), entries.end(), [](auto &a, auto &b) {
      std::error_code x, y;
      bool ad = fs::is_directory(a, x), bd = fs::is_directory(b, y);
      return ad != bd ? ad > bd : a.filename().string() < b.filename().string();
    });
    paths.push_back(path.parent_path());
    list->add("../");
    for (auto &entry : entries) {
      paths.push_back(entry);
      list->add(
          (entry.filename().string() + (fs::is_directory(entry, e) ? "/" : ""))
              .c_str());
    }
    window->redraw();
  }
  void files() {
    directories = boolean(options, "directory") || method == "SaveFiles";
    multiple = boolean(options, "multiple") && method == "OpenFile";
    window->begin();
    directory = new Fl_Input(100, 90, 580, 30, "Folder:");
    list = new Fl_Multi_Browser(20, 132, 660, 208);
    list->format_char(0);
    list->type(multiple ? FL_MULTI_BROWSER : FL_HOLD_BROWSER);
    filename = new Fl_Input(100, 350, 580, 30,
                            method == "OpenFile" ? "File:" : "Name:");
    filename->value(fs::path(text(options, "current_name")).filename().c_str());
    if (directories)
      filename->deactivate();
    auto folder = g_variant_lookup_value(options, "current_folder",
                                         G_VARIANT_TYPE_BYTESTRING);
    auto file = g_variant_lookup_value(options, "current_file",
                                       G_VARIANT_TYPE_BYTESTRING);
    auto initial = bytes(folder);
    auto initial_file = bytes(file);
    if (initial.empty())
      initial = g_get_home_dir();
    if (!initial_file.empty()) {
      initial = fs::path(initial_file).parent_path();
      filename->value(fs::path(initial_file).filename().c_str());
    }
    if (folder)
      g_variant_unref(folder);
    if (file)
      g_variant_unref(file);
    directory->value(initial.c_str());
    directory->when(FL_WHEN_ENTER_KEY_ALWAYS);
    directory->callback(
        [](Fl_Widget *, void *p) { static_cast<Request *>(p)->load(); }, this);
    filename->when(FL_WHEN_CHANGED);
    filename->callback(
        [](Fl_Widget *, void *p) {
          static_cast<Request *>(p)->overwrite_confirmed = false;
        },
        this);
    list->callback(
        [](Fl_Widget *, void *p) {
          auto r = static_cast<Request *>(p);
          int selected = r->list->value();
          if (selected < 1 || size_t(selected) > r->paths.size())
            return;
          auto path = r->paths[selected - 1];
          std::error_code e;
          if (Fl::event_clicks() && fs::is_directory(path, e)) {
            r->directory->value(path.c_str());
            r->load();
          } else if (!fs::is_directory(path, e)) {
            r->filename->value(path.filename().c_str());
            r->overwrite_confirmed = false;
          }
        },
        this);
    auto serialized =
        g_variant_lookup_value(options, "filters", G_VARIANT_TYPE("a(sa(us))"));
    if (serialized && g_variant_n_children(serialized)) {
      filters = new Fl_Choice(100, 388, 580, 28, "Type:");
      auto current = g_variant_lookup_value(options, "current_filter",
                                            G_VARIANT_TYPE("(sa(us))"));
      int selected = 0;
      for (gsize i = 0; i < g_variant_n_children(serialized); ++i) {
        auto f = g_variant_get_child_value(serialized, i);
        filter_data.push_back(f);
        filters->add(menu_label(string_at(f, 0)).c_str());
        if (current && g_variant_equal(f, current))
          selected = i;
      }
      if (current)
        g_variant_unref(current);
      filters->value(selected);
      filters->callback(
          [](Fl_Widget *, void *p) { static_cast<Request *>(p)->load(); },
          this);
    }
    if (serialized)
      g_variant_unref(serialized);
    window->end();
    add_choices();
    auto label =
        text(options, "accept_label", method == "OpenFile" ? "Open" : "Save");
    accept->copy_label(label.c_str());
    action = [this] {
      std::vector<fs::path> selected;
      std::error_code e;
      if (method == "SaveFiles") {
        fs::path parent(directory->value());
        if (int row = list->value(); row > 1 && size_t(row) <= paths.size())
          parent = paths[row - 1];
        if (!fs::is_directory(parent, e)) {
          error("Select a destination folder.");
          return;
        }
        auto names =
            g_variant_lookup_value(options, "files", G_VARIANT_TYPE("aay"));
        if (!names) {
          error("The application supplied no filenames.");
          return;
        }
        std::set<fs::path> used;
        for (gsize i = 0; i < g_variant_n_children(names); ++i) {
          auto name = g_variant_get_child_value(names, i);
          auto base = fs::path(bytes(name)).filename();
          g_variant_unref(name);
          if (base.empty() || base == "." || base == "..")
            continue;
          auto path = parent / base;
          unsigned suffix = 1;
          while (fs::exists(path, e) || used.count(path))
            path = parent /
                   (base.stem().string() + " (" + std::to_string(suffix++) +
                    ")" + base.extension().string());
          used.insert(path);
          selected.push_back(path);
        }
        g_variant_unref(names);
      } else if (method == "SaveFile") {
        auto name = fs::path(filename->value());
        if (name.empty() || name.has_parent_path() || name == "." ||
            name == "..") {
          error("Enter a filename without a folder path.");
          return;
        }
        auto path = fs::path(directory->value()) / name;
        if (fs::is_directory(path, e)) {
          error("That name is a folder.");
          return;
        }
        if (fs::exists(path, e) && !overwrite_confirmed) {
          overwrite_confirmed = true;
          error("This file exists. Choose Save again to replace it.");
          return;
        }
        selected.push_back(path);
      } else {
        for (int row = 2; row <= list->size(); ++row)
          if (list->selected(row))
            selected.push_back(paths.at(row - 1));
        if (selected.empty() && directories)
          selected.push_back(fs::path(directory->value()));
        if (selected.empty() && *filename->value())
          selected.push_back(fs::path(directory->value()) / filename->value());
        for (auto &p : selected)
          if (!fs::exists(p, e) || (directories != fs::is_directory(p, e))) {
            error(directories
                      ? "Choose a folder."
                      : "Choose a file; double-click a folder to open it.");
            return;
          }
      }
      if (selected.empty()) {
        error("Select at least one file.");
        return;
      }
      if (!multiple && method != "SaveFiles" && selected.size() > 1)
        selected.resize(1);
      GVariantBuilder uris, results;
      g_variant_builder_init(&uris, G_VARIANT_TYPE("as"));
      g_variant_builder_init(&results, G_VARIANT_TYPE_VARDICT);
      for (auto &path : selected) {
        auto absolute = fs::absolute(path, e).lexically_normal();
        char *uri = g_filename_to_uri(absolute.c_str(), nullptr, nullptr);
        if (uri) {
          g_variant_builder_add(&uris, "s", uri);
          g_free(uri);
        }
      }
      g_variant_builder_add(&results, "{sv}", "uris",
                            g_variant_builder_end(&uris));
      g_variant_builder_add(&results, "{sv}", "writable",
                            g_variant_new_boolean(TRUE));
      results_choices(results);
      if (filters)
        g_variant_builder_add(&results, "{sv}", "current_filter",
                              filter_data.at(filters->value()));
      finish(0, g_variant_builder_end(&results));
    };
    load();
    show();
  }
  void update_apps(GVariant *ids) {
    applications.clear();
    list->clear();
    for (gsize n = 0; n < g_variant_n_children(ids); ++n) {
      auto id = string_at(ids, n);
      auto desktop = g_desktop_app_info_new(id.c_str());
      applications.push_back(id);
      list->add(desktop ? g_app_info_get_display_name(G_APP_INFO(desktop))
                        : id.c_str());
      if (desktop)
        g_object_unref(desktop);
    }
    list->value(0);
  }
};
static const GDBusInterfaceVTable request_vtable = {
    [](GDBusConnection *, const gchar *sender, const gchar *, const gchar *,
       const gchar *, GVariant *, GDBusMethodInvocation *reply, gpointer data) {
      if (!trusted(sender, reply))
        return;
      auto request = static_cast<Request *>(data)->shared_from_this();
      request->finish(1);
      g_dbus_method_invocation_return_value(reply, nullptr);
    },
    nullptr, nullptr};
static std::shared_ptr<Request>
request(GVariant *args, const std::string &method, std::string title,
        GDBusMethodInvocation *reply, size_t option_index) {
  auto handle = string_at(args, 0);
  auto options = g_variant_get_child_value(args, option_index);
  if (requests.count(handle)) {
    g_variant_unref(options);
    fail(reply, "Request handle already in use");
    return {};
  }
  if (requests.size() >= 32) {
    g_variant_unref(options);
    fail(reply, "Too many pending requests");
    return {};
  }
  auto r = std::make_shared<Request>(handle, string_at(args, 1), method, title,
                                     options, reply);
  g_variant_unref(options);
  GError *error = nullptr;
  r->registration = g_dbus_connection_register_object(
      bus, handle.c_str(), xml.at("Request")->interfaces[0], &request_vtable,
      r.get(), nullptr, &error);
  if (!r->registration) {
    fail(reply, error->message);
    g_error_free(error);
    Fl::delete_widget(r->window);
    r->window = nullptr;
    return {};
  }
  requests[handle] = r;
  return r;
}
struct PrintToken {
  std::string app, printer, copies, duplex;
  gint64 expires;
};
static std::map<guint, PrintToken> print_tokens;
static void print_file(std::shared_ptr<Request> r, PrintToken setting) {
  if (r->fd < 0) {
    r->error("The application supplied no printable file.");
    return;
  }
  GError *error = nullptr;
  auto launch = g_subprocess_launcher_new(GSubprocessFlags(
      G_SUBPROCESS_FLAGS_STDOUT_PIPE | G_SUBPROCESS_FLAGS_STDERR_PIPE));
  g_subprocess_launcher_take_fd(launch, dup(r->fd), STDIN_FILENO);
  auto title = std::string(r->message->label());
  r->process = g_subprocess_launcher_spawn(
      launch, &error, "/usr/bin/lp", "-d", setting.printer.c_str(), "-t",
      title.c_str(), "-n", setting.copies.c_str(), "-o", setting.duplex.c_str(),
      nullptr);
  g_object_unref(launch);
  if (!r->process) {
    r->error(error->message);
    g_error_free(error);
    return;
  }
  r->accept->deactivate();
  r->error("Submitting the print job…");
  g_subprocess_communicate_utf8_async(
      r->process, nullptr, nullptr,
      [](GObject *source, GAsyncResult *result, gpointer p) {
        auto holder = static_cast<std::shared_ptr<Request> *>(p);
        auto r = *holder;
        delete holder;
        gchar *out = nullptr, *err = nullptr;
        GError *error = nullptr;
        bool ok = g_subprocess_communicate_utf8_finish(
                      G_SUBPROCESS(source), result, &out, &err, &error) &&
                  g_subprocess_get_successful(G_SUBPROCESS(source));
        if (!r->done) {
          if (ok)
            r->finish(0);
          else {
            r->error(error ? error->message
                           : (err && *err ? err : "Printing failed."));
            r->accept->activate();
          }
        }
        g_clear_error(&error);
        g_free(out);
        g_free(err);
        g_clear_object(&r->process);
      },
      new std::shared_ptr<Request>(r));
}
static void print_dialog(std::shared_ptr<Request> r, GVariant *settings,
                         GVariant *page_setup) {
  auto retained_settings =
      settings ? g_variant_ref(settings) : g_variant_ref_sink(empty_dict());
  auto retained_page =
      page_setup ? g_variant_ref(page_setup) : g_variant_ref_sink(empty_dict());
  // Retain variants for the lifetime of the accept callback without leaking on
  // cancellation.
  auto state = std::shared_ptr<GVariant>(retained_settings, g_variant_unref),
       page = std::shared_ptr<GVariant>(retained_page, g_variant_unref);
  r->window->begin();
  auto printers = new Fl_Choice(180, 110, 500, 32, "Printer:");
  auto copies = new Fl_Int_Input(180, 160, 100, 32, "Copies:");
  copies->value(text(settings, "n-copies", "1").c_str());
  auto duplex = new Fl_Choice(180, 210, 500, 32, "Sides:");
  duplex->add("One-sided|Two-sided, long edge|Two-sided, short edge");
  duplex->value(0);
  r->window->end();
  std::vector<std::string> names;
  GError *error = nullptr;
  gchar *out = nullptr, *err = nullptr;
  auto command =
      g_subprocess_new(GSubprocessFlags(G_SUBPROCESS_FLAGS_STDOUT_PIPE |
                                        G_SUBPROCESS_FLAGS_STDERR_PIPE),
                       &error, "/usr/bin/lpstat", "-e", nullptr);
  if (command) {
    g_subprocess_communicate_utf8(command, nullptr, nullptr, &out, &err,
                                  &error);
    g_object_unref(command);
  }
  if (out) {
    std::istringstream stream(out);
    std::string line;
    while (std::getline(stream, line))
      if (!line.empty()) {
        names.push_back(line);
        printers->add(menu_label(line).c_str());
      }
  }
  g_free(out);
  g_free(err);
  g_clear_error(&error);
  auto wanted = text(settings, "printer");
  int selected = 0;
  for (size_t i = 0; i < names.size(); ++i)
    if (names[i] == wanted)
      selected = i;
  printers->value(selected);
  r->accept->copy_label(r->method == "PreparePrint" ? "Continue" : "Print");
  if (names.empty()) {
    r->error("No printers are configured. Add a printer through CUPS first.");
    r->accept->deactivate();
  }
  std::weak_ptr<Request> weak = r;
  r->action = [weak, printers, copies, duplex, names, state, page] {
    auto r = weak.lock();
    if (!r)
      return;
    char *end = nullptr;
    long n = strtol(copies->value(), &end, 10);
    if (!end || *end || n < 1 || n > 999) {
      r->error("Copies must be between 1 and 999.");
      return;
    }
    const char *sides[] = {"sides=one-sided", "sides=two-sided-long-edge",
                           "sides=two-sided-short-edge"};
    PrintToken token{r->app, names.at(printers->value()), std::to_string(n),
                     sides[duplex->value()],
                     g_get_monotonic_time() + 600 * G_USEC_PER_SEC};
    if (r->method == "Print") {
      print_file(r, token);
      return;
    }
    guint id;
    do {
      id = g_random_int();
    } while (!id || print_tokens.count(id));
    for (auto i = print_tokens.begin(); i != print_tokens.end();) {
      if (i->second.expires < g_get_monotonic_time())
        i = print_tokens.erase(i);
      else
        ++i;
    }
    print_tokens[id] = token;
    GVariantDict settings;
    g_variant_dict_init(&settings, state.get());
    g_variant_dict_insert(&settings, "printer", "s", token.printer.c_str());
    g_variant_dict_insert(&settings, "n-copies", "s", token.copies.c_str());
    g_variant_dict_insert(&settings, "duplex", "s",
                          duplex->value() == 0   ? "simplex"
                          : duplex->value() == 1 ? "vertical"
                                                 : "horizontal");
    GVariantBuilder result;
    g_variant_builder_init(&result, G_VARIANT_TYPE_VARDICT);
    g_variant_builder_add(&result, "{sv}", "settings",
                          g_variant_dict_end(&settings));
    g_variant_builder_add(&result, "{sv}", "page-setup", page.get());
    g_variant_builder_add(&result, "{sv}", "token", g_variant_new_uint32(id));
    r->finish(0, g_variant_builder_end(&result));
  };
  r->show();
}
struct Notice {
  std::string app, id;
  std::shared_ptr<GVariant> data;
};
static std::map<guint, Notice> notices;
struct Inhibition {
  guint registration;
  int fd;
};
static std::map<std::string, Inhibition> inhibitions;
static std::map<std::string, guint> monitors;
static void monitor_state(bool idle, guint state) {
  for (auto &[path, registration] : monitors) {
    (void)registration;
    GVariantBuilder values;
    g_variant_builder_init(&values, G_VARIANT_TYPE_VARDICT);
    g_variant_builder_add(&values, "{sv}", "screensaver-active",
                          g_variant_new_boolean(idle));
    g_variant_builder_add(&values, "{sv}", "session-state",
                          g_variant_new_uint32(state));
    g_dbus_connection_emit_signal(
        bus, frontend.c_str(), "/org/freedesktop/portal/desktop",
        "org.freedesktop.impl.portal.Inhibit", "StateChanged",
        g_variant_new("(o@a{sv})", path.c_str(),
                      g_variant_builder_end(&values)),
        nullptr);
  }
}
static const GDBusInterfaceVTable inhibition_vtable = {
    [](GDBusConnection *, const gchar *sender, const gchar *path, const gchar *,
       const gchar *, GVariant *, GDBusMethodInvocation *reply, gpointer) {
      if (!trusted(sender, reply))
        return;
      auto item = inhibitions.find(path);
      if (item != inhibitions.end()) {
        close(item->second.fd);
        g_dbus_connection_unregister_object(bus, item->second.registration);
        inhibitions.erase(item);
      }
      auto session = monitors.find(path);
      if (session != monitors.end()) {
        g_dbus_connection_emit_signal(bus, frontend.c_str(), path,
                                      "org.freedesktop.impl.portal.Session",
                                      "Closed", nullptr, nullptr);
        g_dbus_connection_unregister_object(bus, session->second);
        monitors.erase(session);
      }
      g_dbus_method_invocation_return_value(reply, nullptr);
    },
    nullptr, nullptr};
static void inhibit_method(const std::string &method, GVariant *args,
                           GDBusMethodInvocation *reply) {
  if (method == "QueryEndResponse") {
    g_dbus_method_invocation_return_value(reply, nullptr);
    return;
  }
  if (method == "CreateMonitor") {
    auto path = string_at(args, 1);
    if (monitors.count(path)) {
      fail(reply, "Monitor already exists");
      return;
    }
    GError *error = nullptr;
    auto registration = g_dbus_connection_register_object(
        bus, path.c_str(), xml.at("Session")->interfaces[0], &inhibition_vtable,
        nullptr, nullptr, &error);
    if (!registration) {
      fail(reply, error->message);
      g_error_free(error);
      return;
    }
    monitors[path] = registration;
    g_dbus_method_invocation_return_value(reply, g_variant_new("(u)", 0u));
    monitor_state(false, 1);
    return;
  }
  auto path = string_at(args, 0), app = string_at(args, 1);
  guint flags;
  g_variant_get_child(args, 3, "u", &flags);
  if ((flags & 3) || !flags || (flags & ~15u)) {
    g_dbus_method_invocation_return_dbus_error(
        reply, "org.freedesktop.portal.Error.NotSupported",
        "DomeD supports sleep and idle inhibitors; logout and user-switch "
        "inhibition require compositor integration");
    return;
  }
  if (inhibitions.count(path)) {
    fail(reply, "Inhibitor already exists");
    return;
  }
  std::string what;
  if (flags & 4)
    what = "sleep";
  if (flags & 8) {
    if (!what.empty())
      what += ":";
    what += "idle";
  }
  auto options = g_variant_get_child_value(args, 4);
  auto reason = text(options, "reason", "Application activity");
  g_variant_unref(options);
  GError *error = nullptr;
  auto system = g_bus_get_sync(G_BUS_TYPE_SYSTEM, nullptr, &error);
  GUnixFDList *fds = nullptr;
  auto value =
      system ? g_dbus_connection_call_with_unix_fd_list_sync(
                   system, "org.freedesktop.login1", "/org/freedesktop/login1",
                   "org.freedesktop.login1.Manager", "Inhibit",
                   g_variant_new("(ssss)", what.c_str(), app.c_str(),
                                 reason.c_str(), "block"),
                   G_VARIANT_TYPE("(h)"), G_DBUS_CALL_FLAGS_NONE, 5000, nullptr,
                   &fds, nullptr, &error)
             : nullptr;
  if (system)
    g_object_unref(system);
  if (!value) {
    fail(reply, error ? error->message : "Cannot contact logind");
    g_clear_error(&error);
    return;
  }
  gint index;
  g_variant_get(value, "(h)", &index);
  int fd = g_unix_fd_list_get(fds, index, &error);
  g_variant_unref(value);
  g_object_unref(fds);
  if (fd < 0) {
    fail(reply, error->message);
    g_error_free(error);
    return;
  }
  auto registration = g_dbus_connection_register_object(
      bus, path.c_str(), xml.at("Request")->interfaces[0], &inhibition_vtable,
      nullptr, nullptr, &error);
  if (!registration) {
    close(fd);
    fail(reply, error->message);
    g_error_free(error);
    return;
  }
  inhibitions[path] = {registration, fd};
  g_dbus_method_invocation_return_value(reply, nullptr);
}
static void notification(const std::string &method, GVariant *args,
                         GDBusMethodInvocation *reply) {
  auto app = string_at(args, 0), id = string_at(args, 1);
  guint old = 0;
  for (auto &[key, value] : notices)
    if (value.app == app && value.id == id)
      old = key;
  if (method == "RemoveNotification") {
    if (old) {
      g_dbus_connection_call(
          bus, "org.freedesktop.Notifications",
          "/org/freedesktop/Notifications", "org.freedesktop.Notifications",
          "CloseNotification", g_variant_new("(u)", old), nullptr,
          G_DBUS_CALL_FLAGS_NONE, -1, nullptr, nullptr, nullptr);
      notices.erase(old);
    }
    g_dbus_method_invocation_return_value(reply, nullptr);
    return;
  }
  auto data = g_variant_get_child_value(args, 2);
  GVariantBuilder actions, hints;
  g_variant_builder_init(&actions, G_VARIANT_TYPE("as"));
  g_variant_builder_init(&hints, G_VARIANT_TYPE_VARDICT);
  auto default_action = text(data, "default-action");
  if (!default_action.empty()) {
    g_variant_builder_add(&actions, "s", "default");
    g_variant_builder_add(&actions, "s", "");
  }
  auto buttons =
      g_variant_lookup_value(data, "buttons", G_VARIANT_TYPE("aa{sv}"));
  if (buttons) {
    for (gsize n = 0; n < g_variant_n_children(buttons); ++n) {
      auto b = g_variant_get_child_value(buttons, n);
      g_variant_builder_add(&actions, "s", std::to_string(n).c_str());
      g_variant_builder_add(&actions, "s", text(b, "label").c_str());
      g_variant_unref(b);
    }
    g_variant_unref(buttons);
  }
  g_variant_builder_add(&hints, "{sv}", "desktop-entry",
                        g_variant_new_string(app.c_str()));
  auto body = text(data, "body"), title = text(data, "title");
  bool markup = false;
  auto capabilities = g_dbus_connection_call_sync(
      bus, "org.freedesktop.Notifications", "/org/freedesktop/Notifications",
      "org.freedesktop.Notifications", "GetCapabilities", nullptr,
      G_VARIANT_TYPE("(as)"), G_DBUS_CALL_FLAGS_NONE, 2000, nullptr, nullptr);
  if (capabilities) {
    auto values = g_variant_get_child_value(capabilities, 0);
    for (gsize i = 0; i < g_variant_n_children(values); ++i)
      markup |= string_at(values, i) == "body-markup";
    g_variant_unref(values);
    g_variant_unref(capabilities);
  }
  auto escaped = markup ? g_markup_escape_text(body.c_str(), body.size())
                        : g_strdup(body.c_str());
  auto params = g_variant_new(
      "(susss@as@a{sv}i)", app.c_str(), old, "", title.c_str(), escaped,
      g_variant_builder_end(&actions), g_variant_builder_end(&hints), -1);
  g_free(escaped);
  struct Pending {
    GDBusMethodInvocation *reply;
    Notice notice;
    guint old;
  };
  auto pending =
      new Pending{G_DBUS_METHOD_INVOCATION(g_object_ref(reply)),
                  {app, id, std::shared_ptr<GVariant>(data, g_variant_unref)},
                  old};
  g_dbus_connection_call(
      bus, "org.freedesktop.Notifications", "/org/freedesktop/Notifications",
      "org.freedesktop.Notifications", "Notify", params, G_VARIANT_TYPE("(u)"),
      G_DBUS_CALL_FLAGS_NONE, 5000, nullptr,
      [](GObject *object, GAsyncResult *result, gpointer p) {
        auto pending = static_cast<Pending *>(p);
        GError *error = nullptr;
        auto value = g_dbus_connection_call_finish(G_DBUS_CONNECTION(object),
                                                   result, &error);
        if (value) {
          guint id;
          g_variant_get(value, "(u)", &id);
          notices.erase(pending->old);
          notices[id] = pending->notice;
          g_variant_unref(value);
          g_dbus_method_invocation_return_value(pending->reply, nullptr);
        } else {
          fail(pending->reply, error->message);
          g_error_free(error);
        }
        g_object_unref(pending->reply);
        delete pending;
      },
      pending);
}
static void method(GDBusConnection *, const gchar *sender, const gchar *,
                   const gchar *interface, const gchar *method_name,
                   GVariant *args, GDBusMethodInvocation *reply, gpointer) {
  if (!trusted(sender, reply))
    return;
  std::string type = interface, name = method_name;
  if (type.ends_with("Inhibit")) {
    inhibit_method(name, args, reply);
    return;
  }
  if (type.ends_with("Notification")) {
    notification(name, args, reply);
    return;
  }
  if (type.ends_with("AppChooser") && name == "UpdateChoices") {
    auto i = requests.find(string_at(args, 0));
    if (i != requests.end() && i->second->list) {
      auto choices = g_variant_get_child_value(args, 1);
      i->second->update_apps(choices);
      g_variant_unref(choices);
    }
    g_dbus_method_invocation_return_value(reply, nullptr);
    return;
  }
  if (type.ends_with("DynamicLauncher") && name == "RequestInstallToken") {
    g_dbus_method_invocation_return_value(reply, g_variant_new("(u)", 2u));
    return;
  }
  size_t options = g_variant_n_children(args) - 1;
  std::string title = "Application request";
  if (type.ends_with("FileChooser") || type.ends_with("Access") ||
      type.ends_with("Print"))
    title = string_at(args, 3);
  auto r = request(args, name, title, reply, options);
  if (!r)
    return;
  if (type.ends_with("FileChooser")) {
    r->files();
    return;
  }
  if (type.ends_with("Access")) {
    r->window->begin();
    auto body = new Fl_Box(20, 95, 660, 300);
    body->align(FL_ALIGN_INSIDE | FL_ALIGN_LEFT | FL_ALIGN_TOP | FL_ALIGN_WRAP);
    body->copy_label(
        (string_at(args, 4) + "\n\n" + string_at(args, 5)).c_str());
    r->window->end();
    r->add_choices();
    r->cancel->copy_label(text(r->options, "deny_label", "Deny").c_str());
    r->accept->copy_label(text(r->options, "grant_label", "Allow").c_str());
    r->action = [p = r.get()] {
      GVariantBuilder result;
      g_variant_builder_init(&result, G_VARIANT_TYPE_VARDICT);
      p->results_choices(result);
      p->finish(0, g_variant_builder_end(&result));
    };
    r->show();
    return;
  }
  if (type.ends_with("AppChooser")) {
    r->window->begin();
    r->list = new Fl_Multi_Browser(20, 100, 660, 400);
    r->list->format_char(0);
    r->list->type(FL_HOLD_BROWSER);
    r->window->end();
    auto choices = g_variant_get_child_value(args, 3);
    r->update_apps(choices);
    g_variant_unref(choices);
    r->accept->copy_label("Choose");
    r->action = [p = r.get()] {
      int row = p->list->value();
      if (row < 1 || size_t(row) > p->applications.size()) {
        p->error("Choose an application.");
        return;
      }
      GVariantBuilder result;
      g_variant_builder_init(&result, G_VARIANT_TYPE_VARDICT);
      g_variant_builder_add(
          &result, "{sv}", "choice",
          g_variant_new_string(p->applications[row - 1].c_str()));
      p->finish(0, g_variant_builder_end(&result));
    };
    r->show();
    return;
  }
  if (type.ends_with("Print")) {
    if (name == "PreparePrint") {
      auto settings = g_variant_get_child_value(args, 4),
           page = g_variant_get_child_value(args, 5);
      print_dialog(r, settings, page);
      g_variant_unref(settings);
      g_variant_unref(page);
    } else {
      gint index;
      g_variant_get_child(args, 4, "h", &index);
      auto list = g_dbus_message_get_unix_fd_list(
          g_dbus_method_invocation_get_message(reply));
      if (list)
        r->fd = g_unix_fd_list_get(list, index, nullptr);
      guint id = 0;
      g_variant_lookup(r->options, "token", "u", &id);
      auto i = print_tokens.find(id);
      if (i != print_tokens.end() && i->second.app == r->app &&
          i->second.expires > g_get_monotonic_time()) {
        auto token = i->second;
        print_tokens.erase(i);
        print_file(r, token);
      } else
        print_dialog(r, nullptr, nullptr);
    }
    return;
  }
  if (type.ends_with("Account")) {
    r->message->copy_label(
        (r->app + " wants your account name. Share it?").c_str());
    r->accept->copy_label("Share");
    r->action = [p = r.get()] {
      auto user = getpwuid(getuid());
      std::string name = user ? user->pw_gecos : g_get_user_name();
      name = name.substr(0, name.find(','));
      GVariantBuilder result;
      g_variant_builder_init(&result, G_VARIANT_TYPE_VARDICT);
      g_variant_builder_add(&result, "{sv}", "id",
                            g_variant_new_string(g_get_user_name()));
      g_variant_builder_add(&result, "{sv}", "name",
                            g_variant_new_string(name.c_str()));
      g_variant_builder_add(&result, "{sv}", "image", g_variant_new_string(""));
      p->finish(0, g_variant_builder_end(&result));
    };
    r->show();
    return;
  }
  if (type.ends_with("DynamicLauncher")) {
    auto name = string_at(args, 3);
    auto icon = g_variant_get_child_value(args, 4);
    auto saved = std::shared_ptr<GVariant>(icon, g_variant_unref);
    r->window->begin();
    auto preview = new IconPreview(20, 165, icon);
    auto label = new Fl_Input(100, 110, 580, 32, "Name:");
    label->value(name.c_str());
    if (!boolean(r->options, "editable_name", true))
      label->deactivate();
    auto target = new Fl_Box(135, 170, 545, 160);
    target->align(FL_ALIGN_INSIDE | FL_ALIGN_LEFT | FL_ALIGN_WRAP);
    target->copy_label(
        ("Install an application launcher?\n" + text(r->options, "target"))
            .c_str());
    r->window->end();
    r->accept->copy_label("Install");
    if (!preview->valid()) {
      r->error("The launcher needs a valid PNG or SVG icon before it can be "
               "installed.");
      r->accept->deactivate();
    }
    r->action = [p = r.get(), label, saved] {
      if (!*label->value()) {
        p->error("Enter a launcher name.");
        return;
      }
      GVariantBuilder result;
      g_variant_builder_init(&result, G_VARIANT_TYPE_VARDICT);
      g_variant_builder_add(&result, "{sv}", "name",
                            g_variant_new_string(label->value()));
      g_variant_builder_add(&result, "{sv}", "icon", saved.get());
      p->finish(0, g_variant_builder_end(&result));
    };
    r->show();
    return;
  }
  if (type.ends_with("Email")) {
    r->message->copy_label(
        (r->app + " wants to open a message in Penguin Mail.").c_str());
    r->accept->copy_label("Compose");
    r->action = [p = r.get()] {
      std::vector<std::string> command{"/usr/bin/xdg-email", "--utf8"};
      for (auto key : {"subject", "body"}) {
        auto value = text(p->options, key);
        if (!value.empty()) {
          command.push_back(std::string("--") + key);
          command.push_back(value);
        }
      }
      for (auto key : {"cc", "bcc", "attachments"}) {
        auto values =
            g_variant_lookup_value(p->options, key, G_VARIANT_TYPE("as"));
        if (values) {
          for (gsize n = 0; n < g_variant_n_children(values); ++n) {
            command.push_back(
                std::string("--") +
                (std::string(key) == "attachments" ? "attach" : key));
            command.push_back(string_at(values, n));
          }
          g_variant_unref(values);
        }
      }
      auto addresses =
          g_variant_lookup_value(p->options, "addresses", G_VARIANT_TYPE("as"));
      if (addresses) {
        for (gsize n = 0; n < g_variant_n_children(addresses); ++n) {
          auto address = string_at(addresses, n);
          if (address.starts_with("-")) {
            g_variant_unref(addresses);
            p->error("Invalid recipient address.");
            return;
          }
          command.push_back(address);
        }
        g_variant_unref(addresses);
      } else {
        auto address = text(p->options, "address");
        if (address.starts_with("-")) {
          p->error("Invalid recipient address.");
          return;
        }
        if (!address.empty())
          command.push_back(address);
      }
      std::vector<const char *> argv;
      for (auto &a : command)
        argv.push_back(a.c_str());
      argv.push_back(nullptr);
      GError *error = nullptr;
      auto child =
          g_subprocess_newv(argv.data(), G_SUBPROCESS_FLAGS_NONE, &error);
      if (!child) {
        p->error(error->message);
        g_error_free(error);
        return;
      }
      g_object_unref(child);
      p->finish(0);
    };
    r->show();
    return;
  }
  r->finish(2);
}
static GVariant *property(GDBusConnection *, const gchar *, const gchar *,
                          const gchar *interface, const gchar *name, GError **,
                          gpointer) {
  if (!strcmp(name, "version"))
    return g_variant_new_uint32(1);
  if (!strcmp(name, "SupportedLauncherTypes"))
    return g_variant_new_uint32(3);
  if (!strcmp(name, "SupportedOptions"))
    return empty_dict();
  if (std::string(interface).ends_with("Lockdown"))
    return g_variant_new_boolean(FALSE);
  return nullptr;
}
static const GDBusInterfaceVTable vtable = {method, property, nullptr};
int main(int argc, char **argv) try {
  if (argc > 1 && !strcmp(argv[1], "--help")) {
    puts("xdg-desktop-portal-fltk — native DomeD FLTK portal backend");
    return 0;
  }
  struct rlimit limit{0, 0};
  setrlimit(RLIMIT_CORE, &limit);
  dx::init();
  GError *error = nullptr;
  bus = g_bus_get_sync(G_BUS_TYPE_SESSION, nullptr, &error);
  if (!bus)
    throw std::runtime_error(error->message);
  const char *directory = getenv("DOME_PORTAL_INTERFACES");
  if (!directory)
    directory = fs::is_directory("/usr/local/share/domed/portal-interfaces")
                    ? "/usr/local/share/domed/portal-interfaces"
                    : DOME_PORTAL_INTERFACES;
  for (auto name : {"FileChooser", "Access", "AppChooser", "Print",
                    "Notification", "Email", "Account", "DynamicLauncher",
                    "Lockdown", "Inhibit", "Request", "Session"}) {
    std::ifstream file(fs::path(directory) / (std::string(name) + ".xml"));
    std::string contents((std::istreambuf_iterator<char>(file)), {});
    auto info = g_dbus_node_info_new_for_xml(contents.c_str(), &error);
    if (!info)
      throw std::runtime_error(error->message);
    xml[name] = info;
    if (std::string(name) != "Request" && std::string(name) != "Session") {
      if (!g_dbus_connection_register_object(
              bus, "/org/freedesktop/portal/desktop", info->interfaces[0],
              &vtable, nullptr, nullptr, &error))
        throw std::runtime_error(error->message);
    }
  }
  g_dbus_connection_signal_subscribe(
      bus, "org.freedesktop.Notifications", "org.freedesktop.Notifications",
      "ActionInvoked", "/org/freedesktop/Notifications", nullptr,
      G_DBUS_SIGNAL_FLAGS_NONE,
      [](GDBusConnection *, const gchar *, const gchar *, const gchar *,
         const gchar *, GVariant *args, gpointer) {
        guint id;
        const char *action;
        g_variant_get(args, "(u&s)", &id, &action);
        auto i = notices.find(id);
        if (i == notices.end())
          return;
        auto &n = i->second;
        std::string command;
        GVariant *target = nullptr;
        if (!strcmp(action, "default")) {
          command = text(n.data.get(), "default-action");
          target = g_variant_lookup_value(n.data.get(), "default-action-target",
                                          nullptr);
        } else {
          char *end = nullptr;
          auto index = strtoul(action, &end, 10);
          auto buttons = g_variant_lookup_value(n.data.get(), "buttons",
                                                G_VARIANT_TYPE("aa{sv}"));
          if (end && !*end && buttons &&
              index < g_variant_n_children(buttons)) {
            auto button = g_variant_get_child_value(buttons, index);
            command = text(button, "action");
            target = g_variant_lookup_value(button, "target", nullptr);
            g_variant_unref(button);
          }
          if (buttons)
            g_variant_unref(buttons);
        }
        if (command.empty()) {
          if (target)
            g_variant_unref(target);
          return;
        }
        GVariantBuilder params;
        g_variant_builder_init(&params, G_VARIANT_TYPE("av"));
        if (target) {
          g_variant_builder_add(&params, "v", target);
          g_variant_unref(target);
        }
        g_dbus_connection_emit_signal(
            bus, frontend.c_str(), "/org/freedesktop/portal/desktop",
            "org.freedesktop.impl.portal.Notification", "ActionInvoked",
            g_variant_new("(sss@av)", n.app.c_str(), n.id.c_str(),
                          command.c_str(), g_variant_builder_end(&params)),
            nullptr);
      },
      nullptr, nullptr);
  auto owned = g_dbus_connection_call_sync(
      bus, "org.freedesktop.DBus", "/org/freedesktop/DBus",
      "org.freedesktop.DBus", "RequestName",
      g_variant_new("(su)", "org.freedesktop.impl.portal.desktop.fltk", 4u),
      G_VARIANT_TYPE("(u)"), G_DBUS_CALL_FLAGS_NONE, -1, nullptr, &error);
  guint code = 0;
  if (owned) {
    g_variant_get(owned, "(u)", &code);
    g_variant_unref(owned);
  }
  if (code != 1)
    throw std::runtime_error("FLTK portal name is already owned");
  g_dbus_connection_signal_subscribe(
      bus, "org.freedesktop.DBus", "org.freedesktop.DBus", "NameOwnerChanged",
      "/org/freedesktop/DBus", "org.freedesktop.portal.Desktop",
      G_DBUS_SIGNAL_FLAGS_NONE,
      [](GDBusConnection *, const gchar *, const gchar *, const gchar *,
         const gchar *, GVariant *args, gpointer) {
        const char *name, *old, *next;
        g_variant_get(args, "(&s&s&s)", &name, &old, &next);
        (void)name;
        if (*old && strcmp(old, next)) {
          while (!requests.empty())
            requests.begin()->second->finish(1);
          for (auto &[path, item] : inhibitions) {
            (void)path;
            close(item.fd);
            g_dbus_connection_unregister_object(bus, item.registration);
          }
          inhibitions.clear();
          for (auto &[path, id] : monitors) {
            (void)path;
            g_dbus_connection_unregister_object(bus, id);
          }
          monitors.clear();
          print_tokens.clear();
          notices.clear();
          frontend = next;
        }
      },
      nullptr, nullptr);
  auto system = g_bus_get_sync(G_BUS_TYPE_SYSTEM, nullptr, nullptr);
  if (system) {
    auto session_id = login_session::id();
    auto session = g_dbus_connection_call_sync(
        system, "org.freedesktop.login1", "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
        session_id.empty() ? "GetSessionByPID" : "GetSession",
        session_id.empty() ? g_variant_new("(u)", guint(getpid()))
                           : g_variant_new("(s)", session_id.c_str()),
        G_VARIANT_TYPE("(o)"), G_DBUS_CALL_FLAGS_NONE, 2000, nullptr, nullptr);
    if (session) {
      auto path = string_at(session, 0);
      g_dbus_connection_signal_subscribe(
          system, "org.freedesktop.login1", "org.freedesktop.DBus.Properties",
          "PropertiesChanged", path.c_str(), "org.freedesktop.login1.Session",
          G_DBUS_SIGNAL_FLAGS_NONE,
          [](GDBusConnection *, const gchar *, const gchar *, const gchar *,
             const gchar *, GVariant *args, gpointer) {
            auto changes = g_variant_get_child_value(args, 1);
            gboolean idle;
            if (g_variant_lookup(changes, "IdleHint", "b", &idle))
              monitor_state(idle, 1);
            g_variant_unref(changes);
          },
          nullptr, nullptr);
      g_variant_unref(session);
    }
    g_dbus_connection_signal_subscribe(
        system, "org.freedesktop.login1", "org.freedesktop.login1.Manager",
        "PrepareForShutdown", "/org/freedesktop/login1", nullptr,
        G_DBUS_SIGNAL_FLAGS_NONE,
        [](GDBusConnection *, const gchar *, const gchar *, const gchar *,
           const gchar *, GVariant *args, gpointer) {
          gboolean ending;
          g_variant_get(args, "(b)", &ending);
          monitor_state(false, ending ? 2 : 1);
        },
        nullptr, nullptr);
  }
  signal(SIGTERM, [](int) { running = false; });
  signal(SIGINT, [](int) { running = false; });
  puts("DomeD FLTK portal ready");
  fflush(stdout);
  while (running) {
    while (g_main_context_iteration(nullptr, FALSE)) {
    }
    Fl::wait(.02);
    if (!Fl::first_window())
      g_usleep(20000);
  }
  while (!requests.empty())
    requests.begin()->second->finish(1);
  if (system)
    g_object_unref(system);
  g_object_unref(bus);
  return 0;
} catch (const std::exception &e) {
  fprintf(stderr, "FLTK portal: %s\n", e.what());
  return 1;
}
