// SPDX-License-Identifier: MIT
#include <FL/Fl_Double_Window.H>
#include <FL/Fl_Box.H>
#include <FL/Fl.H>
int main(int argc,char **argv) {
  Fl_Double_Window window(320,200,argc>1?argv[1]:"Desktop window fixture");
  Fl_Box text(12,12,296,176,"Virtual desktop test window");
  window.end();window.xclass("org.domed.WorkspaceFixture");window.show();
  return Fl::run();
}
