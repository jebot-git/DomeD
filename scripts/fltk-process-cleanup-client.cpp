// SPDX-License-Identifier: MIT
// Real toolkit lifecycle: own all widgets before closing the process display.
#include "fltk_appearance.h"
#include <FL/Fl_Box.H>
#include <FL/Fl_Double_Window.H>
#include <FL/Fl_Gl_Window.H>
#include <FL/platform.H>
#include <FL/gl.h>
#include <chrono>
#include <iostream>
#include <stdexcept>

namespace {
unsigned checks=0;
void check(bool value,const char *message) {
  if(!value)throw std::runtime_error(message);
  ++checks;std::cout<<"PASS "<<message<<std::endl;
}
struct Fonts {
  Fonts(){if(!FcInit())throw std::runtime_error("Fontconfig initialization failed");}
  ~Fonts(){FcFini();}
};
struct ProcessDisplay {
  // No FLTK calls may follow this documented, final process operation.
  ~ProcessDisplay(){fl_close_display();}
};
struct View:Fl_Gl_Window {
  unsigned frames=0;
  View():Fl_Gl_Window(0,0,320,160) {
    mode(FL_RGB|FL_DOUBLE|FL_DEPTH|FL_OPENGL3);end();
  }
  void draw() override {
    glViewport(0,0,pixel_w(),pixel_h());
    glClearColor(.1f,.2f,.3f,1);glClear(GL_COLOR_BUFFER_BIT|GL_DEPTH_BUFFER_BIT);
    if(!glGetString(GL_RENDERER)||glGetError()!=GL_NO_ERROR)
      throw std::runtime_error("Native GL context is invalid");
    if(!frames)std::cout<<"GL_RENDERER "<<glGetString(GL_RENDERER)<<std::endl;
    ++frames;
  }
};
}

int main() try {
  Fonts fonts;
  ProcessDisplay display;
  dx::init();
  {
    Fl_Double_Window window(320,220,"DomeD process cleanup fixture");
    View view;
    Fl_Box label(0,160,320,60,"Bitmap text and native GL");
    window.end();window.resizable(view);
    for(unsigned cycle=0;cycle<3;++cycle) {
      auto before=view.frames;window.show();view.redraw();
      auto deadline=std::chrono::steady_clock::now()+std::chrono::seconds(10);
      while(view.frames==before&&std::chrono::steady_clock::now()<deadline)Fl::wait(.01);
      check(view.frames>before&&view.context(),"Actual native GL context draws after show");
      for(unsigned i=0;i<10;++i)Fl::wait(.01);
      {
        Fl_Image_Surface surface(64,64);
        Fl_Surface_Device::push_current(&surface);
        // Region ownership and the Cairo Clip root both survive until driver
        // destruction. This exposed leaks beyond the original EGL report.
        fl_clip_region(fl_graphics_driver->XRectangleRegion(2,2,40,40));
        fl_color(FL_RED);fl_rectf(0,0,64,64);
        fl_push_clip(4,4,24,24);fl_font(dx::font,dx::size);
        fl_color(FL_WHITE);fl_draw("M",6,24);fl_pop_clip();
        auto image=surface.image();
        check(image&&image->w()==64&&image->h()==64,
              "Native bitmap text and owned clipping region render on the image surface");
        delete image;Fl_Surface_Device::pop_current();
      }
      window.hide();
      check(!view.context(),"Hide releases the actual native GL context before another show");
    }
  }
  if(const char *path=std::getenv("DOME_FLTK_CLEANUP_MAPS")) {
    std::ifstream in("/proc/self/maps");std::ofstream out(path);out<<in.rdbuf();
    if(!out)throw std::runtime_error("Cannot retain runtime library provenance");
  }
  std::cout<<checks<<" native process cleanup checks passed"<<std::endl;
  return 0;
}catch(const std::exception &error){std::cerr<<error.what()<<std::endl;return 1;}
