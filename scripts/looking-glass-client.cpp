// SPDX-License-Identifier: MIT
#include "../native/fltk_appearance.h"
#include <FL/Fl_Double_Window.H>
#include <FL/fl_draw.H>
#include <cstdio>
#include <memory>
#include <vector>
struct Content : Fl_Widget {
  int index, tick = 0;
  Content(int i) : Fl_Widget(0, 0, 540, 340), index(i) {}
  void draw() override {
    fl_color(fl_rgb_color(35 + index * 25, 65 + index * 12, 95 + index * 12));
    fl_rectf(x(), y(), w(), h());
    for (int row = 0; row < 5; ++row)
      for (int col = 0; col < 8; ++col) {
        fl_color(fl_rgb_color(65 + col * 20, 75 + row * 24, 100 + index * 15));
        fl_rectf(x() + col * w() / 8, y() + row * h() / 5, w() / 8 - 2,
                 h() / 5 - 2);
      }
    fl_color(FL_WHITE);
    fl_font(FL_COURIER, 16);
    fl_draw(("Native live window " + std::to_string(index + 1) + " / frame " +
             std::to_string(tick))
                .c_str(),
            20, 40);
    fl_color(FL_YELLOW);
    fl_rectf(20 + (tick * 17) % 400, 90, 65, 65);
  }
  int handle(int event) override {
    if (event == FL_FOCUS || event == FL_UNFOCUS)
      return 1;
    if (event == FL_PUSH) {
      take_focus();
      return 1;
    }
    if (event == FL_KEYDOWN) {
      std::printf("KEY %s\n", Fl::event_text());
      std::fflush(stdout);
      return 1;
    }
    return 0;
  }
};
int main() {
  dx::init();
  std::vector<Content *> contents;
  for (int i = 0; i < 8; ++i) {
    auto title = "Looking Glass client " + std::to_string(i + 1);
    auto win = new Fl_Double_Window(540, 340);
    win->copy_label(title.c_str());
    auto id = "org.domed.LookingGlassFixture." + std::to_string(i + 1);
    win->xclass(id.c_str());
    contents.push_back(new Content(i));
    win->end();
    win->resizable(contents.back());
    win->show();
    contents.back()->take_focus();
  }
  struct Timer {
    static void run(void *p) {
      auto contents = static_cast<std::vector<Content *> *>(p);
      for (auto content : *contents) {
        ++content->tick;
        content->redraw();
      }
      Fl::repeat_timeout(.2, run, p);
    }
  };
  Fl::add_timeout(.2, Timer::run, &contents);
  return Fl::run();
}
