// SPDX-License-Identifier: MIT
#include <FL/Fl.H>
#include <FL/Fl_Input.H>
#include <FL/Fl_Window.H>
#include <FL/Fl_Box.H>
#include <FL/Fl_Copy_Surface.H>
#include <FL/Fl_RGB_Image.H>
#include <FL/fl_draw.H>
#include <FL/platform.H>
#include "../../native/bdf_font.hpp"
#include <X11/Xlib.h>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <functional>
#include <vector>
#include <fcntl.h>
#include <unistd.h>
#include <iostream>
#include <stdexcept>
#include <string>
extern Window fl_message_window;
namespace fs = std::filesystem;
static unsigned checks;
static void require(bool value,const char *message){if(!value)throw std::runtime_error(message);++checks;}
struct Descriptors {
  std::vector<int> values;
  Descriptors(){if(!getenv("DOME_CLIPBOARD_HIGH_FD"))return;values.reserve(1101);for(int i=0;i<1101;++i){int fd=open("/dev/null",O_RDONLY|O_CLOEXEC);if(fd<0){for(int value:values)close(value);throw std::runtime_error("High descriptor test reservation failed");}values.push_back(fd);if(fd>=1100)break;}}
  ~Descriptors(){for(int value:values)close(value);}
};
struct Fonts {
  std::string name;
  Fonts(){require(FcInit(),"Fontconfig init");auto path=bdf_fonts::export_config();setenv("FONTCONFIG_FILE",path.c_str(),1);FcInitReinitialize();auto face=bdf_fonts::resolve("Unifont","Sans-Serif",16,1);name=face.family+" / "+face.style;Fl::set_font(FL_HELVETICA,name.c_str());FL_NORMAL_SIZE=16;}
  ~Fonts(){Fl::do_widget_deletion();fl_close_display();FcFini();}
};
struct ImageReceiver:Fl_Box {
  bool exact=false;
  ImageReceiver():Fl_Box(0,0,1,1){}
  int handle(int event) override {if(event!=FL_PASTE)return 0;auto image=static_cast<Fl_RGB_Image *>(Fl::event_clipboard());if(Fl::event_clipboard_type()==Fl::clipboard_image&&image&&image->w()==2&&image->h()==1&&image->d()>=3){auto p=reinterpret_cast<const unsigned char *>(image->data()[0]);int d=image->d();exact=p[0]==255&&p[1]==0&&p[2]==0&&p[d]==0&&p[d+1]==255&&p[d+2]==0;}return 0;}
};
static void until(const std::function<bool()> &ready,double seconds=2){auto end=std::chrono::steady_clock::now()+std::chrono::duration<double>(seconds);while(!ready()){if(std::chrono::steady_clock::now()>=end)throw std::runtime_error("Clipboard protocol observation timeout");Fl::wait(.005);}}
struct Events {
  unsigned count=0;Atom marker=0;
  static int observe(void *raw,void *data){auto &self=*static_cast<Events*>(data);auto &event=*static_cast<XEvent*>(raw);if(event.type==ClientMessage&&event.xclient.message_type==self.marker)++self.count;return 0;}
  Events(){marker=XInternAtom(fl_display,"DOME_CLIPBOARD_PRESERVED_EVENT",False);Fl::add_system_handler(observe,this);}
  ~Events(){Fl::remove_system_handler(observe);}
};
int main(int argc,char **argv) try {
  require(argc==4,"mode output external-owner");const std::string mode=argv[1];fs::path out=argv[2];fs::create_directories(out);const Window external=std::stoul(argv[3]);Descriptors descriptors;Fonts fonts;
  {Fl_Window window(360,180,"Clipboard protocol probe");Fl_Input input(20,60,320,40);input.value("sentinel");window.end();window.show();input.take_focus();Fl::flush();Events events;if(getenv("DOME_CLIPBOARD_HIGH_FD"))require(ConnectionNumber(fl_display)>1100,"Actual display descriptor exceeds select fd_set capacity");std::cout<<"DISPLAY_FD="<<ConnectionNumber(fl_display)<<"\nREADY\n"<<std::flush;until([&]{return fs::exists(out/"armed");},10);
    if(mode=="ownership"){
      const std::string first="native first";Fl::copy(first.c_str(),first.size(),1);Atom selection=XInternAtom(fl_display,"CLIPBOARD",False);require(XGetSelectionOwner(fl_display,selection)==fl_message_window,"Explicit native copy claims actual ownership after newer external timestamp");input.insert_position(input.size(),0);Fl::paste(input,1);until([&]{return input.value()==first;});require(input.value()==first,"Own native paste uses exact copied bytes");
      std::cout<<"RECLAIM\n"<<std::flush;until([&]{return fs::exists(out/"reclaimed");});const std::string second="native second";Fl::copy(second.c_str(),second.size(),1);require(XGetSelectionOwner(fl_display,selection)==fl_message_window&&fl_message_window!=external,"Repeated source owner handoff claims genuine native window");input.insert_position(input.size(),0);Fl::paste(input,1);until([&]{return input.value()==second;});require(input.value()==second,"Second native copy/paste exact");
      std::cout<<"RECLAIM_IMAGE\n"<<std::flush;until([&]{return fs::exists(out/"image-reclaimed");});{Fl_Copy_Surface image(2,1);Fl_Surface_Device::push_current(&image);fl_color(255,0,0);fl_rectf(0,0,1,1);fl_color(0,255,0);fl_rectf(1,0,1,1);Fl_Surface_Device::pop_current();}require(XGetSelectionOwner(fl_display,selection)==fl_message_window,"Native image copy claims actual newer ownership");require(Fl::clipboard_contains(Fl::clipboard_image)&&!Fl::clipboard_contains(Fl::clipboard_plain_text),"Native image target inventory remains distinct");ImageReceiver receiver;Fl::paste(receiver,1,Fl::clipboard_image);require(receiver.exact,"Public copy surface/image paste retains exact pixels and ownership");
    }else if(mode=="normal"){
      require(Fl::clipboard_contains(Fl::clipboard_plain_text),"External text target available");input.insert_position(input.size(),0);Fl::paste(input,1);require(Fl::clipboard_contains(Fl::clipboard_plain_text),"Concurrent metadata query preserves real paste reply");until([&]{return std::string(input.value())=="external exact payload";});require(std::string(input.value())=="external exact payload","Real external paste payload exact");until([&]{return events.count>=3;});require(events.count==3,"All unrelated native events delivered exactly once");
    }else if(mode=="malformed"){
      for(int i=0;i<20;++i)require(!Fl::clipboard_contains(Fl::clipboard_plain_text),"Wrong TARGETS property type refuses");until([&]{return events.count==20;});require(std::string(input.value())=="sentinel","Metadata never becomes field data");
    }else {
      const auto before=std::chrono::steady_clock::now();require(!Fl::clipboard_contains(Fl::clipboard_plain_text),"Unavailable metadata refuses");const auto elapsed=std::chrono::duration<double>(std::chrono::steady_clock::now()-before).count();require(elapsed<1,"Metadata wait bounded independently of input events");
      if(mode=="late"){const auto end=std::chrono::steady_clock::now()+std::chrono::milliseconds(600);while(std::chrono::steady_clock::now()<end)Fl::wait(.005);require(std::string(input.value())=="sentinel","Late metadata reply never dispatches as real paste");}
      until([&]{return events.count==1;});require(events.count==1,"Refusal/timeout preserves unrelated native event");
    }
    window.hide();
  }
  std::cout<<"PASS clipboard "<<mode<<" checks="<<checks<<'\n';
}catch(const std::exception &e){std::cerr<<e.what()<<'\n';return 1;}
