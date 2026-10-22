// SPDX-License-Identifier: MIT
#pragma once
#include "../fltk_appearance.h"
#include <FL/Fl_Button.H>
#include <FL/Fl_Box.H>
#include <functional>

// A standalone tooltip surface: unlike Fl_Menu_Window it can be created while
// the editor is hidden. DomeD attaches this role to the source window, including
// XR placement; X11 uses FLTK's native tooltip window hint.
class SelectionTip : public Fl_Window {
  class Text : public Fl_Box {
  public:
    using Fl_Box::Fl_Box;
    void draw() override {
      fl_color(labelcolor());fl_font(labelfont(),labelsize());
      fl_draw(label()?label():"",x(),y(),w(),h(),align(),nullptr,0);
    }
  };
  Text body{12,12,456,100};
  Fl_Button copyButton{12,124,140,30,"Copy translation"};
  Fl_Button openButton{162,124,170,30,"Open in translator"};
  Fl_Button closeButton{342,124,126,30,"Close"};
  std::string text_;
  uint64_t fontRevision=0;
  static void expire(void *p) {static_cast<SelectionTip*>(p)->dismiss();}
public:
  std::function<void()> closed,copy,open;
  std::function<void(const std::string&)> pasted;
  std::string translated;
  SelectionTip():Fl_Window(480,166,"Selected text translation") {
    border(0);set_tooltip_window();xclass("org.domed.Translator.Selection");
    body.align(FL_ALIGN_INSIDE|FL_ALIGN_LEFT|FL_ALIGN_TOP|FL_ALIGN_WRAP);
    copyButton.callback([](Fl_Widget*,void*p){auto t=static_cast<SelectionTip*>(p);if(t->copy)t->copy();},this);
    openButton.callback([](Fl_Widget*,void*p){auto t=static_cast<SelectionTip*>(p);if(t->open)t->open();},this);
    closeButton.callback([](Fl_Widget*,void*p){static_cast<SelectionTip*>(p)->dismiss();},this);
    end();callback([](Fl_Widget*,void*p){static_cast<SelectionTip*>(p)->dismiss();},this);
    dx::style(this);
  }
  ~SelectionTip(){Fl::remove_timeout(expire,this);}
  void hide() override {Fl::remove_timeout(expire,this);Fl_Window::hide();}
  void dismiss(){hide();if(closed)closed();}
  void display(const std::string&text,bool ready=false,bool locate=false) {
    text_=text;if(ready){copyButton.activate();openButton.activate();}else{copyButton.deactivate();openButton.deactivate();}
    updateLayout();
    int px=x(),py=y();
    if(locate){Fl::get_mouse(px,py);py+=12;}
    int sx=0,sy=0,sw=1280,sh=800;Fl::screen_work_area(sx,sy,sw,sh,Fl::screen_num(px,py));
    position(std::clamp(px,sx,std::max(sx,sx+sw-w())),std::clamp(py,sy,std::max(sy,sy+sh-h())));
    show();Fl::remove_timeout(expire,this);
    if(ready && !(Fl::belowmouse() && contains(Fl::belowmouse())))Fl::add_timeout(20.,expire,this);
  }
  void updateLayout() {
    dx::style(this);fontRevision=dx::font_revision;
    color(FL_BACKGROUND2_COLOR);body.color(FL_BACKGROUND2_COLOR);body.labelcolor(FL_FOREGROUND_COLOR);
    // Tooltips are bounded; copying/opening always retains the full result.
    std::string visible=text_;if(visible.size()>2400){size_t end=2400;while(end>0&&(visible[end]&0xc0)==0x80)--end;visible.resize(end);visible+="…";}
    body.copy_label(visible.c_str());
    fl_font(dx::font,dx::size);int width=456,height=0;
    fl_measure(visible.c_str(),width,height,0);
    int content=std::clamp(height+8,70,420);
    int row=content+22;
    resize(x(),y(),480,row+42);size_range(w(),h(),w(),h());
    body.resize(12,12,456,content);
    copyButton.resize(12,row,140,30);openButton.resize(162,row,170,30);closeButton.resize(342,row,126,30);
    redraw();
  }
  void draw() override {
    if(fontRevision!=dx::font_revision)updateLayout();
    Fl_Window::draw();fl_color(FL_SELECTION_COLOR);fl_rect(0,0,w(),h());
  }
  int handle(int event) override {
    if(event==FL_PASTE){if(pasted)pasted(std::string(Fl::event_text(),Fl::event_length()));return 1;}
    if(event==FL_ENTER){Fl::remove_timeout(expire,this);}
    if(event==FL_LEAVE&&!translated.empty()){Fl::remove_timeout(expire,this);Fl::add_timeout(20.,expire,this);}
    if((event==FL_KEYDOWN||event==FL_SHORTCUT)&&Fl::event_key()==FL_Escape){dismiss();return 1;}
    return Fl_Window::handle(event);
  }
};
