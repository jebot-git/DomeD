// SPDX-License-Identifier: MIT
#include <FL/Fl.H>
#include <FL/Fl_Button.H>
#include <FL/Fl_Check_Button.H>
#include <FL/Fl_Choice.H>
#include <FL/Fl_File_Browser.H>
#include <FL/Fl_File_Chooser.H>
#include <FL/Fl_File_Input.H>
#include <FL/Fl_Input.H>
#include <FL/Fl_Hold_Browser.H>
#include <FL/Fl_Menu_Button.H>
#include <FL/Fl_Native_File_Chooser.H>
#include <FL/Fl_RGB_Image.H>
#include <FL/Fl_Scroll.H>
#include <FL/platform.H>
#include "../../native/bdf_font.hpp"
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
std::vector<std::unique_ptr<Fl_Widget_Tracker>> theme_windows;
Fl_Font picker_font=FL_FREE_FONT;int picker_pixels=16;std::string picker_font_name;
void style_tree(Fl_Widget *w){w->labelfont(picker_font);w->labelsize(picker_pixels);if(auto p=dynamic_cast<Fl_Input_ *>(w)){p->textfont(picker_font);p->textsize(picker_pixels);}if(auto p=dynamic_cast<Fl_Browser_ *>(w)){p->textfont(picker_font);p->textsize(picker_pixels);}if(auto p=dynamic_cast<Fl_Menu_ *>(w)){p->textfont(picker_font);p->textsize(picker_pixels);}if(auto g=dynamic_cast<Fl_Group *>(w))for(int i=0;i<g->children();++i)style_tree(g->child(i));}
void style_new(void *){for(auto w=Fl::first_window();w;w=Fl::next_window(w)){bool found=false;for(const auto &p:theme_windows)if(!p->deleted()&&p->widget()==w)found=true;if(!found){style_tree(w);theme_windows.emplace_back(new Fl_Widget_Tracker(w));w->redraw();}}}
struct Fonts {
  Fonts(){require(FcInit(),"Process Fontconfig initialization");auto config=bdf_fonts::export_config();setenv("FONTCONFIG_FILE",config.c_str(),1);FcInitReinitialize();int scale=getenv("DOME_PICKER_FONT_SCALE")?std::stoi(getenv("DOME_PICKER_FONT_SCALE")):1;require(scale>=1&&scale<=4,"Bounded test font scale");auto face=bdf_fonts::resolve("Unifont","Sans-Serif",16,scale);picker_pixels=face.pixels;picker_font_name=face.family+" / "+face.style;Fl::set_font(picker_font,picker_font_name.c_str());FL_NORMAL_SIZE=picker_pixels;fl_message_font(picker_font,picker_pixels);setenv("DE_XR_BDF_PIXELS",std::to_string(picker_pixels).c_str(),1);Fl::add_check(style_new);std::cout<<"BITMAP_FONT="<<face.file<<" PIXELS="<<picker_pixels<<std::endl;}
  ~Fonts(){Fl::remove_check(style_new);theme_windows.clear();Fl::do_widget_deletion();fl_close_display();FcFini();}
};
bool contained(Fl_Window &w,Fl_Widget &p){return p.visible_r()&&p.x()>=0&&p.y()>=0&&p.x()+p.w()<=w.w()&&p.y()+p.h()<=w.h();}
void viewport(Fl_Window &w){int X,Y,W,H;Fl::screen_work_area(X,Y,W,H,Fl::screen_num(w.x(),w.y(),w.w(),w.h()));std::cout<<"VIEWPORT window="<<w.x()<<','<<w.y()<<','<<w.w()<<','<<w.h()<<" work="<<X<<','<<Y<<','<<W<<','<<H<<std::endl;if(auto maximum=getenv("DOME_PICKER_MAX_WORK_AREA")){int maxW=0,maxH=0;require(sscanf(maximum,"%dx%d",&maxW,&maxH)==2&&W<=maxW&&H<=maxH,"Actual Wayland work area matches compact nested output");}require(w.x()>=X&&w.y()>=Y&&w.x()+w.w()<=X+W&&w.y()+w.h()<=Y+H,"Chooser must fit actual viewport work area");}
Fl_Button *button(Fl_Window &w,const std::string &name){for(auto p:widgets(w))if(auto b=dynamic_cast<Fl_Button *>(p))if(b->label()&&name==b->label())return b;return nullptr;}
void close_options(){if(auto w=window("Locations"))w->hide();if(auto w=window("File chooser options"))w->hide();settle();}
struct InputPrompt {
  std::string value,error;bool cancel=false,seen=false;int attempts=0;
  static void act(void *data){auto &test=*static_cast<InputPrompt *>(data);try {
    for(auto w=Fl::first_window();w;w=Fl::next_window(w)){
      if(!w->shown())continue;Fl_Input *input=nullptr;for(auto p:widgets(*w))if(auto q=dynamic_cast<Fl_Input *>(p))if(!dynamic_cast<Fl_File_Input *>(p))input=q;
      if(!input)continue;test.seen=true;viewport(*w);require(contained(*w,*input)&&input->h()>=input->textsize()+8,"Shared prompt input fits actual bitmap glyphs");
      input->value(test.value.c_str());auto accept=button(*w,"OK"),cancel=button(*w,"Cancel");require(accept&&cancel&&contained(*w,*accept)&&contained(*w,*cancel),"Shared prompt accept and Cancel visible");
      (test.cancel?cancel:accept)->do_callback();return;
    }
    require(++test.attempts<100,"Real shared input prompt did not appear");Fl::repeat_timeout(.03,act,data);
  }catch(const std::exception &e){test.error=e.what();for(auto w=Fl::first_window();w;w=Fl::next_window(w))if(w->modal()&&w->shown()){w->do_callback();break;}}}
  void start(){Fl::add_timeout(.03,act,this);}
  void check(){Fl::remove_timeout(act,this);require(error.empty(),error.c_str());require(seen,"Actual shared input prompt exercised");}
};
struct Picker {
  Fl_Window &window;
  Fl_File_Chooser &chooser;
  Fl_File_Browser *files = nullptr;
  Fl_Hold_Browser *places = nullptr;
  Fl_File_Input *filename = nullptr;
  Fl_Choice *sort = nullptr, *direction = nullptr;
  Fl_Check_Button *folders = nullptr, *details = nullptr, *hidden = nullptr, *showPlaces = nullptr;
  Fl_Button *refresh = nullptr, *okay = nullptr, *addPlace = nullptr, *managePlaces = nullptr;
  Picker(Fl_Window &w, Fl_File_Chooser &c) : window(w), chooser(c) {
    for (auto p : widgets(w)) {
      if (auto b = dynamic_cast<Fl_File_Browser *>(p))
        files = b;
      if (auto b = dynamic_cast<Fl_File_Input *>(p))
        filename = b;
      if (auto b = dynamic_cast<Fl_Hold_Browser *>(p))
        if (b->label() && std::string(b->label()) == "Places") places = b;
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
        if (label == "Places") showPlaces = b;
      }
      if (auto b = dynamic_cast<Fl_Button *>(p)) {
        std::string label = b->label() ? b->label() : "";
        if (label == "Refresh")
          refresh = b;
        if (label == "OK" || label == "Save")
          okay = b;
        if (label == "Add") addPlace = b;
        if (label == "Manage") managePlaces = b;
      }
    }
    require(files && filename && sort && direction && folders && details && refresh && okay &&
                hidden && places && showPlaces && addPlace && managePlaces,
            "Enhanced shared file-picker controls missing");
  }
  bool openPlaces(){
    if(places->visible_r())return false;
    auto options=button(window,"Options…");if(!options)options=button(window,"…");require(options&&contained(window,*options),"Compact picker has reachable Options");options->do_callback();settle();auto popup=::window("File chooser options");require(popup,"Options opens a real modal window");viewport(*popup);auto locations=button(*popup,"Locations…");if(locations)std::cout<<"LOCATIONS_BOUNDS "<<locations->x()<<','<<locations->y()<<','<<locations->w()<<','<<locations->h()<<" visible="<<locations->visible_r()<<" popup="<<popup->w()<<','<<popup->h()<<std::endl;require(locations&&contained(*popup,*locations),"Locations is reachable in Options");locations->do_callback();settle();auto where=::window("Locations");require(where&&places->visible_r(),"Places has actual compact Locations access");viewport(*where);return true;
  }
  Fl_Window *options(){auto b=button(window,"Options…");if(!b)b=button(window,"…");require(b&&contained(window,*b),"Options is visible");b->do_callback();settle();auto popup=::window("File chooser options");require(popup,"Actual Options modal opens");viewport(*popup);return popup;}
  Fl_Widget *expose(Fl_Window &popup,const std::function<bool(Fl_Widget *)> &match){Fl_Scroll *scroll=nullptr;for(auto p:widgets(popup))if(auto q=dynamic_cast<Fl_Scroll *>(p))scroll=q;require(scroll,"Options uses actual scroll container");Fl_Widget *found=nullptr;for(int i=0;i<scroll->children();++i)if(match(scroll->child(i)))found=scroll->child(i);require(found,"Requested Options control exists");scroll->scroll_to(0,std::max(0,found->y()-scroll->y()+scroll->yposition()));settle();require(contained(popup,*found)&&found->y()>=scroll->y()&&found->y()+found->h()<=scroll->y()+scroll->h(),"Scrolled Options control fully reachable");require(found->h()>=found->labelsize()+8,"Options control fits bitmap glyphs");return found;}
  void options_roundtrip(){std::string name=filename->value();int count=chooser.count(),key=sort->value(),reverse=direction->value();for(int repeat=0;repeat<3;++repeat){auto popup=options();auto locations=button(*popup,"Locations…");require(locations&&contained(*popup,*locations)&&locations->y()==8,"Repeated Options open resets scroll origin");
    std::vector<Fl_Widget *> controls;Fl_Scroll *scroll=nullptr;for(auto q:widgets(*popup))if(auto t=dynamic_cast<Fl_Scroll *>(q))scroll=t;require(scroll,"Options scroll");for(int i=0;i<scroll->children();++i)if(scroll->child(i)!=&scroll->scrollbar&&scroll->child(i)!=&scroll->hscrollbar)controls.push_back(scroll->child(i));require(controls.size()==12,"Every original control has compact access");
    for(auto q:controls)expose(*popup,[&](Fl_Widget *p){return p==q;});
    if(repeat==1)popup->do_callback();else {auto close=button(*popup,"Close");require(close&&contained(*popup,*close),"Options Close remains visible while scrolled");close->do_callback();}settle();require(!popup->shown(),"Owned Options Close and window-close both return");require(name==filename->value()&&count==chooser.count()&&key==sort->value()&&reverse==direction->value(),"Opening, scrolling and closing Options preserves filename, multiselection and preferences");}
  }
  void prompts(const fs::path &root){chooser.type(Fl_File_Chooser::CREATE);auto popup=options();auto newdir=expose(*popup,[](Fl_Widget *p){return p->label()&&std::string(p->label())=="New folder…";});require(newdir->active(),"New folder proxy preserves CREATE activation");InputPrompt create{"New 日本語 folder"};create.start();newdir->do_callback();create.check();settle();require(fs::is_directory(root/create.value)&&fs::path(chooser.directory())==root/create.value,"Actual New folder accepts Unicode and navigates");
    close_options();chooser.directory(root.c_str());popup=options();newdir=expose(*popup,[](Fl_Widget *p){return p->label()&&std::string(p->label())=="New folder…";});InputPrompt cancel{"must-not-create", "", true};cancel.start();newdir->do_callback();cancel.check();require(!fs::exists(root/cancel.value)&&fs::path(chooser.directory())==root,"New folder Cancel leaves directory unchanged");close_options();
    auto filter_action=[&](bool cancelled){auto pop=options();auto q=static_cast<Fl_Choice *>(expose(*pop,[](Fl_Widget *p){auto c=dynamic_cast<Fl_Choice *>(p);return c&&c->find_item(Fl_File_Chooser::custom_filter_label);}));int custom=q->find_index(Fl_File_Chooser::custom_filter_label);require(custom>=0,"Actual custom filter entry");std::string before=chooser.filter();InputPrompt filter{cancelled?"*.bin":"*.txt", "",cancelled};q->value(custom);filter.start();q->do_callback();filter.check();settle();require(std::string(chooser.filter())==(cancelled?before:"*.txt"),"Custom filter accept/Cancel preserves exact result");if(!cancelled)require(q->find_item("*.txt")&&q->text()&&std::string(q->text())=="*.txt","Custom filter synchronizes visible Options menu");close_options();};filter_action(false);filter_action(true);chooser.filter("*");chooser.type(Fl_File_Chooser::MULTI);
  }
  void bounds(){viewport(window);require(contained(window,*filename)&&contained(window,*okay),"Filename and accept remain visible in client bounds");require(filename->h()>=filename->textsize()+14&&okay->h()>=okay->labelsize()+8,"Filename and accept fit actual bitmap glyph height");auto cancel=button(window,"Cancel");require(cancel&&contained(window,*cancel)&&cancel->h()>=cancel->labelsize()+8,"Cancel remains visible and bitmap-scaled");}
  int placeRow(const std::string &name) {
    for (int i=1;i<=places->size();++i) if (name == places->text(i)) return i;
    return 0;
  }
  void place(const std::string &name) {
    bool popup=openPlaces();int row=placeRow(name); require(row != 0, ("Missing place: "+name).c_str());
    places->value(row); places->do_callback(); settle();if(popup)close_options();
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
  int mode;bool cancel=false;
  static void act(void *data) {
    auto &test = *static_cast<NativeCase *>(data);
    auto w = window(test.title.c_str());
    if (!w) {
      Fl::repeat_timeout(.03, act, data);
      return;
    }
    auto c = static_cast<Fl_File_Chooser *>(w->user_data());
    try {
      Picker p(*w,*c);p.bounds();
      if(test.cancel){auto cancel=button(*w,"Cancel");require(cancel&&contained(*w,*cancel),"Native Cancel is visible");cancel->do_callback();return;}
      p.place("Documents");
      require(fs::path(c->directory()) == test.root / "書類 テスト", "Native picker navigates Places");
      p.place("Home");
      if (test.mode == Fl_Native_File_Chooser::BROWSE_SAVE_FILE) {
        auto wanted = (test.root / "new output 日本語.txt").string();
        p.filename->value("");p.filename->take_focus();Fl::copy(wanted.c_str(),int(wanted.size()),1);Fl::paste(*p.filename,1);settle();require(wanted==p.filename->value(),"Native Unicode save clipboard acknowledged");p.bounds();
        p.place("Documents");
        require(fs::path(c->directory()) == test.root / "書類 テスト" &&
                fs::path(p.filename->value()) == test.root / "書類 テスト/new output 日本語.txt",
                "Places preserves a save basename even when a destination folder has that name");
        p.place("Home");
        require(wanted == p.filename->value(), "Places preserves Unicode save filename");
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
  if (std::string(getenv("FLTK_BACKEND") ? getenv("FLTK_BACKEND") : "") != "wayland")
    setenv("XDG_RUNTIME_DIR", (root / "runtime").c_str(), 1);
  else require(getenv("DOME_PICKER_PRIVATE_RUNTIME") && std::string(getenv("DOME_PICKER_PRIVATE_RUNTIME"))=="1",
               "Use the private Wayland fixture driver");
  fs::create_directories(root / "config/gtk-3.0");
  fs::create_directories(root / "config/fsvr");
  fs::create_directories(root / "書類 テスト/new output 日本語.txt");
  fs::create_directories(root / "Desk top");
  fs::create_directories(root / "Native favorite");
  fs::create_directories(root / "Bookmarked folder");
  std::ofstream(root / "config/user-dirs.dirs")
    << "XDG_DOCUMENTS_DIR=\"$HOME/書類 テスト\"\n"
       "XDG_DESKTOP_DIR=\"${HOME}/Desk top\"\n"
       "XDG_DOWNLOAD_DIR=\"$HOME\"\n"
       "XDG_MUSIC_DIR=\"$(touch " << (root/"must-not-execute").string() << ")\"\n";
  auto uri = [](const fs::path &path) {
    std::string result="file://";
    for (unsigned char c : path.string()) {
      if (c == ' ' || c == '%' || c >= 128) {
        char encoded[4]; snprintf(encoded,sizeof(encoded),"%%%02X",unsigned(c)); result+=encoded;
      } else result+=char(c);
    }
    return result;
  };
  std::ofstream(root / "config/gtk-3.0/bookmarks")
    << uri(root/"Bookmarked folder") << " @b日本語 bookmark\n"
    << uri(root/"Bookmarked folder") << " Duplicate bookmark\n"
    << "file://other-host/remote Remote bookmark\n"
       "file:///broken%00path NUL bookmark\n"
       "file:///broken%xxpath Invalid bookmark\n"
       "smb://unmounted/share Unmounted share\n";
  std::ofstream(root / "config/fsvr/places") << uri(root/"Unavailable folder") << " Offline bookmark\n";
  fs::path connected=fs::path(getenv("XDG_RUNTIME_DIR"))/"gvfs/smb-share:server=fixture,share=demo";
  fs::create_directories(connected);
  {
    Fl_Preferences prefs(Fl_Preferences::CORE_USER,"fltk.org","filechooser");
    prefs.set("favorite00",(root/"Native favorite").c_str()); prefs.flush();
  }
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
  Fonts fonts;
  {std::ifstream maps("/proc/self/maps");std::ofstream(out/"loaded-maps.txt")<<maps.rdbuf();}
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
    p.filename->take_focus();const auto copied=(root/"file2.txt").string();Fl::copy(copied.c_str(),int(copied.size()),1);Fl::paste(*p.filename,1);settle();require(copied==p.filename->value(),"Actual native clipboard filename acknowledgment");std::cout<<"ACK_FILENAME="<<p.filename->value()<<std::endl;p.bounds();
    require(p.showPlaces->value(),"Places preference is enabled by default");bool compact_places=p.openPlaces();require(p.places->visible_r(),"Places is accessible by default");
    {std::string name=p.filename->value(),directory=c.directory();int count=c.count();p.places->value(p.placeRow("Folders"));p.places->do_callback();settle();require(name==p.filename->value()&&directory==c.directory()&&count==c.count(),"Places section rows do not navigate or change selection");
      if(compact_places){auto locations=window("Locations");require(locations&&locations->shown(),"Section row leaves owned Locations open");capture(*locations,out/"locations-compact.ppm");auto close=button(*locations,"Close");require(close&&contained(*locations,*close),"Locations Close remains visible");close->do_callback();settle();require(!locations->shown(),"Locations Close returns to Options");auto options=window("File chooser options");require(options&&options->shown(),"Owned Options survives Locations Close");capture(*options,out/"options-compact.ppm");options->do_callback();settle();require(!options->shown(),"Options window-close returns without navigation");require(name==p.filename->value()&&directory==c.directory()&&count==c.count(),"Owned modal Close preserves current filename and selection");}
    }
    require(p.placeRow("Home") && p.placeRow("File System") && p.placeRow("Documents") &&
            p.placeRow("Desktop") && p.placeRow("Native favorite") && p.placeRow("@b日本語 bookmark") &&
            p.placeRow("Offline bookmark") && p.placeRow("demo on fixture"), "Places sources and Unicode labels");
    require(!p.placeRow("Downloads") && !p.placeRow("Duplicate bookmark") && !p.placeRow("Remote bookmark") &&
            !p.placeRow("NUL bookmark") && !p.placeRow("Invalid bookmark") && !p.placeRow("Unmounted share") &&
            !fs::exists(root/"must-not-execute"), "Places deduplicates, rejects malformed URLs and never executes XDG values");
    p.place("@b日本語 bookmark"); require(fs::path(c.directory()) == root/"Bookmarked folder", "Imported bookmark navigation");
    p.place("demo on fixture"); require(fs::path(c.directory()) == connected, "Already-connected network location navigation");
    p.place("Home");
    c.directory((root/"folder2").c_str());p.openPlaces();p.addPlace->do_callback();
    require(p.placeRow("folder2") && !p.addPlace->active(), "Add synchronizes native Favorites and prevents duplicates");
    p.managePlaces->do_callback(); settle();
    auto manage=window("Manage Favorites"); require(manage, "Existing Favorites manager opens");
    Fl_File_Browser *favorites=nullptr; Fl_Button *remove=nullptr,*save=nullptr;
    for (auto widget : widgets(*manage)) {
      if (auto b=dynamic_cast<Fl_File_Browser *>(widget)) favorites=b;
      if (auto b=dynamic_cast<Fl_Button *>(widget)) {
        std::string label=b->label() ? b->label() : "";
        if (label=="X") remove=b;
        if (label=="Save") save=b;
      }
    }
    require(favorites && remove && save,"Native Favorites management controls");
    for (int i=1;i<=favorites->size();++i)
      if (fs::path(favorites->text(i))==root/"folder2") { favorites->value(i); favorites->do_callback(); break; }
    remove->do_callback();auto cancelFavorite=button(*manage,"Cancel");require(cancelFavorite&&contained(*manage,*cancelFavorite),"Favorites Cancel fits viewport");cancelFavorite->do_callback();settle();require(p.placeRow("folder2"),"Favorites Cancel preserves saved Favorite and Places");p.managePlaces->do_callback();settle();bool selectedFavorite=false;for(int i=1;i<=favorites->size();++i)if(fs::path(favorites->text(i))==root/"folder2"){favorites->value(i);favorites->do_callback();selectedFavorite=true;break;}require(selectedFavorite,"Cancelled Favorite deletion is restored on reopening manager");
    remove->do_callback(); save->do_callback(); settle();
    require(!p.placeRow("folder2"), "Removing a native Favorite refreshes Places");close_options();
    p.place("Home");
    c.directory((root/"missing-folder").c_str()); settle();
    bool error_places=p.openPlaces();require(p.places->visible_r(),"Places remains usable when the file listing fails");if(error_places)close_options();
    p.place("Home"); require(p.files->visible_r() && p.row("file2.txt"), "Places recovers from an unavailable directory");
    p.showPlaces->value(0); p.showPlaces->do_callback(); settle();
    require(!p.places->visible_r(), "Places can be hidden");
    p.showPlaces->value(1); p.showPlaces->do_callback(); settle();
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
    if(p.refresh->visible_r()){require(p.refresh->x()+p.refresh->w()<=w->w(),"Visible toolbar fits minimum width");require(p.showPlaces->x()+p.showPlaces->w()<=p.refresh->x(),"Places toggle fits visible toolbar");}
    p.options_roundtrip();
    {const auto old_font=picker_font;const auto old_pixels=picker_pixels;auto other=bdf_fonts::resolve("Unifont","Sans-Serif",16,old_pixels==16?2:1);static std::string other_name;other_name=other.family+" / "+other.style;Fl::set_font(Fl_Font(FL_FREE_FONT+1),other_name.c_str());std::string name=p.filename->value();int count=c.count();
      for(int phase=0;phase<2;++phase){picker_font=phase?old_font:Fl_Font(FL_FREE_FONT+1);picker_pixels=phase?old_pixels:other.pixels;style_tree(w);w->redraw();settle();p.bounds();auto popup=p.options();auto close=button(*popup,"Close");require(close&&contained(*popup,*close)&&close->h()>=close->labelsize()+8,"Font change reflows existing Options footer");close->do_callback();settle();require(name==p.filename->value()&&count==c.count(),"Actual bitmap font reflow preserves filename and multiselection");}
    }
    c.preview(1);
    settle();
    require(p.files->x() + p.files->w() <= w->w(), "Preview and details must resize together");
    p.bounds();if(p.places->visible_r())require(p.places->x()+p.places->w()<p.files->x(),"Visible sidebar and files do not overlap");else {p.openPlaces();close_options();}
    capture(*w, out / "picker-narrow.ppm");
    c.preview(0);
    w->size(960, 600);
    p.folders->value(1);
    p.folders->do_callback();
    p.order(1, true);
    capture(*w, out / "picker-details.ppm");
    p.showPlaces->value(0); p.showPlaces->do_callback();
    c.hide();
  }
  {
    Fl_File_Chooser c(root.c_str(), "*", Fl_File_Chooser::CREATE, "Prompt controls");c.show();settle();Picker p(*window("Prompt controls"),c);p.prompts(root);c.hide();
  }
  {
    Fl_File_Chooser c(root.c_str(), "*", 0, "Persisted picker");
    c.show();
    settle();
    Picker p(*window("Persisted picker"), c);
    require(p.sort->value() == 1 && p.direction->value() == 1 && p.folders->value() == 1 &&
                p.details->value() == 1,
            "Sort and details preferences must survive chooser destruction");
    require(!p.showPlaces->value() && !p.places->visible_r(), "Places visibility persists across chooser instances");
    p.showPlaces->value(1); p.showPlaces->do_callback();
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
  {
    NativeCase test{"Native picker Cancel","",root,Fl_Native_File_Chooser::BROWSE_FILE,true};Fl_Native_File_Chooser c;c.title(test.title.c_str());c.type(test.mode);c.directory(root.c_str());Fl::add_timeout(.1,NativeCase::act,&test);int result=c.show();Fl::remove_timeout(NativeCase::act,&test);require(test.error.empty(),test.error.c_str());require(result==1,"Actual native picker Cancel returns cancellation");
  }
  std::cout
      << "PASS shared chooser natural/size/type/date sorting, direction, parent/folder order, "
         "hidden files, persistent settings, selection/filename preservation, preview/resizing "
         "Places/XDG/Favorites/bookmarks/connected locations, safe parsing, error recovery "
         "and native open/multiple/save/directory results; ABI size="
      << sizeof(Fl_File_Chooser) << '\n';
} catch (const std::exception &e) {
  std::cerr << e.what() << '\n';
  return 1;
}
