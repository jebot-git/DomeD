// SPDX-License-Identifier: MIT
#include <FL/Fl.H>
#include <FL/Fl_Button.H>
#include <FL/Fl_Check_Button.H>
#include <FL/Fl_Choice.H>
#include <FL/Fl_File_Browser.H>
#include <FL/Fl_File_Chooser.H>
#include <FL/Fl_File_Input.H>
#include <FL/Fl_Native_File_Chooser.H>
#include <FL/Fl_RGB_Image.H>
#include <FL/Fl_Window.H>
#include <FL/fl_draw.H>
#include <algorithm>
#include <cstdlib>
#include <fcntl.h>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iostream>
#include <memory>
#include <set>
#include <stdexcept>
#include <string>
#include <sys/stat.h>
#include <unistd.h>
#include <vector>
namespace fs = std::filesystem;
void require(bool okay, const char *message) {
  if (!okay)
    throw std::runtime_error(message);
}
void settle() {
  for (int i = 0; i < 12; ++i)
    Fl::wait(.02);
}
std::vector<Fl_Widget *> widgets(Fl_Group &group) {
  std::vector<Fl_Widget *> result;
  for (int i = 0; i < group.children(); ++i) {
    auto child = group.child(i);
    result.push_back(child);
    if (auto g = dynamic_cast<Fl_Group *>(child)) {
      auto nested = widgets(*g);
      result.insert(result.end(), nested.begin(), nested.end());
    }
  }
  return result;
}
Fl_Window *window(const char *label) {
  for (auto w = Fl::first_window(); w; w = Fl::next_window(w))
    if (w->label() && std::string(w->label()) == label)
      return w;
  return nullptr;
}
struct Picker {
  Fl_Window &window;
  Fl_File_Chooser &chooser;
  Fl_File_Browser *files = nullptr;
  Fl_File_Input *filename = nullptr;
  Fl_Choice *sort = nullptr, *direction = nullptr;
  Fl_Check_Button *folders = nullptr, *details = nullptr, *hidden = nullptr;
  Fl_Button *refresh = nullptr, *okay = nullptr;
  Picker(Fl_Window &w, Fl_File_Chooser &c) : window(w), chooser(c) {
    for (auto p : widgets(w)) {
      if (auto b = dynamic_cast<Fl_File_Browser *>(p))
        files = b;
      if (auto b = dynamic_cast<Fl_File_Input *>(p))
        filename = b;
      if (auto b = dynamic_cast<Fl_Choice *>(p)) {
        if (b->find_item("Modified"))
          sort = b;
        if (b->find_item("Descending"))
          direction = b;
      }
      if (auto b = dynamic_cast<Fl_Check_Button *>(p)) {
        std::string label = b->label() ? b->label() : "";
        if (label == "Folders first")
          folders = b;
        if (label == "Details")
          details = b;
        if (label == "Show hidden files")
          hidden = b;
      }
      if (auto b = dynamic_cast<Fl_Button *>(p)) {
        std::string label = b->label() ? b->label() : "";
        if (label == "Refresh")
          refresh = b;
        if (label == "OK" || label == "Save")
          okay = b;
      }
    }
    require(files && filename && sort && direction && folders && details && refresh && okay &&
                hidden,
            "Enhanced shared file-picker controls missing");
  }
  int row(const std::string &name) {
    for (int i = 1; i <= files->size(); ++i)
      if (name == files->text(i))
        return i;
    throw std::runtime_error("Missing picker row: " + name);
  }
  void order(int key, bool reverse = false) {
    sort->value(key);
    direction->value(reverse);
    sort->do_callback();
  }
  void heading(const std::string &name) {
    for (auto p : widgets(window))
      if (auto b = dynamic_cast<Fl_Button *>(p))
        if (b->label() && std::string(b->label()).rfind(name + " ", 0) == 0) {
          b->do_callback();
          return;
        }
    throw std::runtime_error("Active column heading missing");
  }
  void checkParent() { require(std::string(files->text(1)) == "../", "Parent must stay first"); }
};
void capture(Fl_Window &window, const fs::path &path) {
  settle();
  Fl::flush();
  std::unique_ptr<Fl_RGB_Image> image(fl_capture_window(&window, 0, 0, window.w(), window.h()));
  require(bool(image), "Cannot capture chooser");
  std::ofstream out(path, std::ios::binary);
  out << "P6\n" << image->w() << ' ' << image->h() << "\n255\n";
  int stride = image->ld() ? image->ld() : image->w() * image->d();
  for (int y = 0; y < image->h(); ++y)
    for (int x = 0; x < image->w(); ++x)
      out.write(image->data()[0] + y * stride + x * image->d(), 3);
}
struct NativeCase {
  std::string title, error;
  fs::path root;
  int mode;
  static void act(void *data) {
    auto &test = *static_cast<NativeCase *>(data);
    auto w = window(test.title.c_str());
    if (!w) {
      Fl::repeat_timeout(.03, act, data);
      return;
    }
    auto c = static_cast<Fl_File_Chooser *>(w->user_data());
    try {
      Picker p(*w, *c);
      if (test.mode == Fl_Native_File_Chooser::BROWSE_SAVE_FILE) {
        auto wanted = (test.root / "new output 日本語.txt").string();
        c->value(wanted.c_str());
        p.order(1, true);
        require(wanted == p.filename->value(), "Sorting changed a typed save filename");
      } else if (test.mode == Fl_Native_File_Chooser::BROWSE_DIRECTORY) {
        c->value((test.root / "folder2").c_str());
      } else {
        c->value((test.root / "file2.txt").c_str());
        if (test.mode == Fl_Native_File_Chooser::BROWSE_MULTI_FILE) {
          p.files->select(p.row("český 日本語.txt"), 1);
          p.order(3, true);
          require(c->count() == 2, "Native multiselection lost while sorting");
        }
        p.files->do_callback();
      }
      p.okay->do_callback();
    } catch (const std::exception &e) {
      test.error = e.what();
      c->hide();
    }
  }
};
int main(int argc, char **argv) try {
  require(argc == 2, "Expected capture directory");
  fs::path out = fs::absolute(argv[1]);
  fs::create_directories(out);
  char temporary[] = "/tmp/dome-file-picker-XXXXXX";
  require(mkdtemp(temporary), "Temporary files");
  fs::path root = temporary;
  struct Cleanup {
    fs::path p;
    ~Cleanup() {
      std::error_code ec;
      fs::remove_all(p, ec);
    }
  } cleanup{root};
  setenv("HOME", temporary, 1);
  setenv("XDG_CONFIG_HOME", (root / "config").c_str(), 1);
  fs::create_directories(root / "folder2");
  fs::create_directories(root / "folder10");
  std::ofstream(root / "file2.txt") << std::string(30, 'x');
  std::ofstream(root / "file10.bin") << "small";
  std::ofstream(root / "český 日本語.txt") << "Unicode";
  std::ofstream(root / ".hidden") << "hidden";
  timespec old[2] = {{1000000000, 0}, {1000000000, 0}},
           recent[2] = {{1700000000, 0}, {1700000000, 0}};
  require(!utimensat(AT_FDCWD, (root / "file2.txt").c_str(), old, 0), "Old timestamp");
  require(!utimensat(AT_FDCWD, (root / "file10.bin").c_str(), recent, 0), "New timestamp");
  Fl::option(Fl::OPTION_FNFC_USES_GTK, false);
  Fl::option(Fl::OPTION_FNFC_USES_ZENITY, false);
  Fl::option(Fl::OPTION_FNFC_USES_KDIALOG, false);
  Fl::background(31, 37, 45);
  Fl::background2(22, 28, 35);
  Fl::foreground(231, 235, 241);
  Fl::set_color(FL_SELECTION_COLOR, 0x6b, 0xab, 0xff);
  {
    Fl_File_Chooser c(root.c_str(), "*", Fl_File_Chooser::MULTI, "Shared file picker test");
    c.preview(0);
    c.show();
    settle();
    auto w = window("Shared file picker test");
    require(w, "Chooser window");
    Picker p(*w, c);
    p.order(0);
    p.checkParent();
    require(p.row("folder2/") < p.row("folder10/") && p.row("file2.txt") < p.row("file10.bin"),
            "Natural ordering must compare digit runs");
    require(p.row("folder10/") < p.row("file2.txt"), "Folders-first default");
    p.files->deselect();
    p.files->select(p.row("file2.txt"), 1);
    p.files->select(p.row("český 日本語.txt"), 1);
    p.order(1);
    p.checkParent();
    require(p.row("file10.bin") < p.row("file2.txt"), "Ascending size ordering");
    p.heading("Size");
    p.checkParent();
    require(p.row("file2.txt") < p.row("file10.bin"), "Column click reverses size ordering");
    require(c.count() == 2, "Sorting must preserve all selected files");
    p.order(3);
    require(p.row("file2.txt") < p.row("file10.bin"), "Modified ascending");
    p.order(3, true);
    require(p.row("file10.bin") < p.row("file2.txt"), "Modified descending");
    p.order(2);
    require(p.row("file10.bin") < p.row("file2.txt"), "Extension/type ordering");
    p.folders->value(0);
    p.folders->do_callback();
    p.order(0);
    require(p.row("file2.txt") < p.row("folder2/"), "Folders-first can be disabled");
    p.hidden->value(1);
    p.hidden->do_callback();
    p.order(1, true);
    p.checkParent();
    require(p.row(".hidden") > 0 && p.row("file2.txt") < p.row("file10.bin"),
            "Showing hidden files retains the selected ordering");
    p.hidden->value(0);
    p.hidden->do_callback();
    p.files->deselect();
    p.files->select(p.row("file2.txt"), 1);
    p.files->select(p.row("český 日本語.txt"), 1);
    p.refresh->do_callback();
    require(c.count() == 2, "Refresh preserves multiselection");
    p.details->value(0);
    p.details->do_callback();
    p.details->value(1);
    p.details->do_callback();
    w->size(720, 420);
    settle();
    require(p.refresh->x() + p.refresh->w() <= w->w(), "Controls must fit minimum width");
    c.preview(1);
    settle();
    require(p.files->x() + p.files->w() <= w->w(), "Preview and details must resize together");
    capture(*w, out / "picker-narrow.ppm");
    c.preview(0);
    w->size(960, 600);
    p.folders->value(1);
    p.folders->do_callback();
    p.order(1, true);
    capture(*w, out / "picker-details.ppm");
    c.hide();
  }
  {
    Fl_File_Chooser c(root.c_str(), "*", 0, "Persisted picker");
    c.show();
    settle();
    Picker p(*window("Persisted picker"), c);
    require(p.sort->value() == 1 && p.direction->value() == 1 && p.folders->value() == 1 &&
                p.details->value() == 1,
            "Sort and details preferences must survive chooser destruction");
    c.hide();
  }
  for (int mode :
       {Fl_Native_File_Chooser::BROWSE_FILE, Fl_Native_File_Chooser::BROWSE_MULTI_FILE,
        Fl_Native_File_Chooser::BROWSE_SAVE_FILE, Fl_Native_File_Chooser::BROWSE_DIRECTORY}) {
    NativeCase test{"Native picker " + std::to_string(mode), "", root, mode};
    Fl_Native_File_Chooser c;
    c.title(test.title.c_str());
    c.type(mode);
    c.directory(root.c_str());
    c.filter("All files\t*");
    Fl::add_timeout(.1, NativeCase::act, &test);
    int result = c.show();
    require(test.error.empty(), test.error.c_str());
    require(result == 0, "Native picker accept");
    require(c.count() == (mode == Fl_Native_File_Chooser::BROWSE_MULTI_FILE ? 2 : 1),
            "Native result count");
    if (mode == Fl_Native_File_Chooser::BROWSE_SAVE_FILE)
      require(fs::path(c.filename()) == root / "new output 日本語.txt",
              "Unicode save path must be returned unchanged");
    if (mode == Fl_Native_File_Chooser::BROWSE_MULTI_FILE) {
      std::set<fs::path> got;
      for (int i = 0; i < c.count(); ++i)
        got.insert(c.filename(i));
      require(got == std::set<fs::path>{root / "file2.txt", root / "český 日本語.txt"},
              "Exact native multiselection results");
    }
  }
  std::cout
      << "PASS shared chooser natural/size/type/date sorting, direction, parent/folder order, "
         "hidden files, persistent settings, selection/filename preservation, preview/resizing "
         "and native open/multiple/save/directory results; ABI size="
      << sizeof(Fl_File_Chooser) << '\n';
} catch (const std::exception &e) {
  std::cerr << e.what() << '\n';
  return 1;
}
