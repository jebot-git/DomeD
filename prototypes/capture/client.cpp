// SPDX-License-Identifier: MIT
#include <FL/Fl.H>
#include <FL/Fl_Double_Window.H>
#include <FL/fl_draw.H>
class Client : public Fl_Double_Window {
  void draw() override {
    fl_color(fl_rgb_color(240, 32, 16));
    fl_rectf(0, 0, w() / 2, h());
    fl_color(fl_rgb_color(16, 32, 240));
    fl_rectf(w() / 2, 0, w() - w() / 2, h());
    fl_color(fl_rgb_color(16, 240, 16));
    fl_rectf(0, 0, w(), h() / 4);
  }

public:
  Client() : Fl_Double_Window(640, 320, "Capture stereo fixture") {
    end();
    xclass("org.fsvxr.fsvr.Sbs");
  }
};
int main() {
  Client client;
  client.show();
  return Fl::run();
}
