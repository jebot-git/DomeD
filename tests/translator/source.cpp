// SPDX-License-Identifier: MIT
#include <FL/Fl.H>
#include <FL/Fl_Double_Window.H>
#include <FL/Fl_Box.H>
int main() {
  Fl_Double_Window window(600,360,"Selection source fixture");
  Fl_Box text(15,15,570,330,"Source application for selection popup anchoring");
  window.end();window.xclass("dome.translator.source-fixture");window.show();
  return Fl::run();
}
