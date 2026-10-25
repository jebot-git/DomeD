// SPDX-License-Identifier: MIT
// Verify missing-glyph fallback through actual FLTK CPU and GL pixels at 1x/2x.
#include "../native/fltk_appearance.h"
#include "../native/gpu-window.h"
#include <FL/Fl_Double_Window.H>
#include <ft2build.h>
#include FT_FREETYPE_H
#include <iostream>
#include <stdexcept>
#ifdef GPU_PROBE
struct Probe : dx::GpuWindow {
  std::string text;
  std::vector<unsigned char> pixels = std::vector<unsigned char>(256*128*3);
  Probe() : dx::GpuWindow(256,128) { end(); }
  void draw() override {
    dx::GpuWindow::draw(); draw_begin();
    fl_color(FL_BLACK);fl_rectf(0,0,256,128);fl_font(FL_FREE_FONT,dx::size);
    fl_color(FL_WHITE);fl_draw(text.c_str(),64,90);draw_end();
    glPixelStorei(GL_PACK_ALIGNMENT,1);glReadPixels(0,0,256,128,GL_RGB,GL_UNSIGNED_BYTE,pixels.data());
  }
};
#endif
using Points = std::vector<std::pair<int,int>>;
static void normalize(Points &points) {
  if(points.empty())throw std::runtime_error("Empty rendered glyph");
  int x=points.front().first,y=points.front().second;
  for(auto &p:points){p.first-=x;p.second-=y;}
}
int main(int argc,char **argv) try {
  if(argc!=3)return 2;
  std::filesystem::path fonts=argv[1],out=argv[2];std::filesystem::create_directories(out);
  setenv("XDG_CONFIG_HOME",out.c_str(),1);std::filesystem::create_directories(out/"dome");
  auto conf=out/"fonts.conf";std::ofstream(conf)<<"<fontconfig><dir>"<<fonts.string()<<"</dir></fontconfig>";
  setenv("FONTCONFIG_FILE",conf.c_str(),1);setenv("DE_XR_FONTCONFIG_BASE",conf.c_str(),1);FcInitReinitialize();
  FT_Library library; if(FT_Init_FreeType(&library))return 2;
  Fl_Double_Window window(256,128);window.end();window.show();Fl::check();
#ifdef GPU_PROBE
  Probe probe;probe.show();
#endif
  struct Sample { unsigned cp;const char *family,*file; };
  Sample samples[]={{'A',"Unifont","unifont-17.0.04.bdf"},
    {0x1f600,"Unifont Upper","unifont-upper-17.0.04/unifont_upper-17.0.04.bdf"},
    {0x1f680,"Unifont Upper","unifont-upper-17.0.04/unifont_upper-17.0.04.bdf"}};
  for(int scale:{1,2}) {
    std::ofstream(out/"dome/appearance.json")<<Json{{"font","Unifont / Sans-Serif"},{"font_size",16},{"font_scale",scale}};
    auto selected=bdf_fonts::resolve("Unifont","Sans-Serif",16,scale);
    std::string name=selected.family+" / "+selected.style;
    Fl::set_font(FL_FREE_FONT,name.c_str());dx::size=16*scale;
    for(auto sample:samples) {
      bdf_fonts::Face source;if(!bdf_fonts::inspect((fonts/sample.file).string(),source))return 2;
      source=bdf_fonts::scaled(source,scale);FT_Face face;
      if(FT_New_Face(library,source.file.c_str(),0,&face))return 2;
      FT_Select_Size(face,0);FT_Select_Charmap(face,FT_ENCODING_UNICODE);
      if(!FT_Get_Char_Index(face,sample.cp)||FT_Load_Char(face,sample.cp,FT_LOAD_DEFAULT))return 2;
      Points expected,actual;auto &bitmap=face->glyph->bitmap;
      for(unsigned y=0;y<bitmap.rows;y++)for(unsigned x=0;x<bitmap.width;x++)
        if(bitmap.buffer[y*bitmap.pitch+x/8]&(128>>(x%8)))expected.emplace_back(x,y);
      char utf8[5]={};int n=fl_utf8encode(sample.cp,utf8);
      Fl_Image_Surface surface(256,128);Fl_Surface_Device::push_current(&surface);
      fl_color(FL_BLACK);fl_rectf(0,0,256,128);fl_font(FL_FREE_FONT,16*scale);
      auto width=fl_width(utf8,n);fl_color(FL_WHITE);fl_draw(utf8,n,64,90);
      auto image=surface.image();Fl_Surface_Device::pop_current();
#ifdef GPU_PROBE
      probe.text=utf8;probe.redraw();for(int i=0;i<3;i++)Fl::wait(.03);
      delete image;std::vector<unsigned char> upright(probe.pixels.size());
      for(int y=0;y<128;y++)std::copy_n(probe.pixels.data()+(127-y)*768,768,upright.data()+y*768);
      image=new Fl_RGB_Image(upright.data(),256,128,3);
#endif
      int stride=image->ld()?image->ld():image->w()*image->d();auto data=(const unsigned char*)image->data()[0];
      for(int y=0;y<128;y++)for(int x=0;x<256;x++) {
        unsigned v=data[y*stride+x*image->d()];if(v!=0&&v!=255)throw std::runtime_error("Filtered bitmap pixel");
        if(v)actual.emplace_back(x,y);
      }
      normalize(expected);normalize(actual);
      if(expected!=actual||width!=face->glyph->advance.x/64.)throw std::runtime_error("Fallback glyph pixels or advance differ");
      std::cout<<"PASS "<<scale<<"x U+"<<std::hex<<sample.cp<<std::dec<<" via "<<sample.family<<" exact mask and advance\n";
      delete image;FT_Done_Face(face);
    }
  }
  window.hide();FT_Done_FreeType(library);return 0;
} catch(const std::exception &e) {std::cerr<<e.what()<<"\n";return 1;}
