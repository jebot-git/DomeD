// SPDX-License-Identifier: MIT
#include <FL/Fl.H>
#include <FL/Fl_Window.H>
#include <FL/fl_draw.H>
#include <cstdio>
class Client : public Fl_Window {
public:
  Client() : Fl_Window(640,400,"Private lock fixture") {xclass("dome-lock-fixture");end();}
  void draw() override {fl_color(220,20,40);fl_rectf(0,0,w(),h());fl_color(FL_WHITE);fl_font(FL_HELVETICA,20);fl_draw("PRIVATE SESSION CONTENT",20,100);}
  int handle(int event) override {
    if(event==FL_KEYDOWN){puts("KEY");fflush(stdout);return 1;}
    if(event==FL_PUSH){puts("BUTTON");fflush(stdout);return 1;}
    return Fl_Window::handle(event);
  }
};
int main(){Client w;w.show();return Fl::run();}
