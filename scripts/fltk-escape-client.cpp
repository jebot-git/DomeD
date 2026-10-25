// SPDX-License-Identifier: MIT
// Exercise FLTK's real shortcut dispatch, including its final Escape fallback.
#include <FL/Fl.H>
#include <FL/Fl_Window.H>
#include <cstdio>
#include <cstdlib>

static int checks;
static void check(bool ok, const char *name) {
  std::printf("%s %s\n", ok ? "PASS" : "FAIL", name);std::fflush(stdout);
  if (!ok) std::exit(1);
  ++checks;
}
static void counted(Fl_Widget *, void *data) { ++*static_cast<int *>(data); }
struct Application : Fl_Window {
  int cancelled = 0, quit = 0;
  bool cancelAction = false;
  Application() : Fl_Window(400, 240, "DomeD Escape application fixture") { end(); }
  int handle(int event) override {
    if (event == FL_SHORTCUT && Fl::event_key() == FL_Escape && cancelAction) {
      ++cancelled; return 1;
    }
    if (event == FL_SHORTCUT && Fl::event_key() == 'q' && Fl::event_state(FL_CTRL)) {
      ++quit; return 1;
    }
    return Fl_Window::handle(event);
  }
};
static int shortcut(Fl_Window *target, int key = FL_Escape, int modifiers = 0) {
  if (target) Fl::first_window(target);
  Fl::focus(nullptr);Fl::belowmouse(nullptr);
  Fl::e_keysym=key;Fl::e_state=modifiers;Fl::e_number=FL_SHORTCUT;
  static char escape[]="\033";Fl::e_text=escape;Fl::e_length=1;
  return Fl::handle(FL_SHORTCUT,target);
}
int main() {
  Application root;int rootCloses=0;root.callback(counted,&rootCloses);root.show();
  for(int i=0;i<5;++i)shortcut(&root);
  check(rootCloses==0&&root.shown(),"Unhandled/repeated Escape leaves root application open");
  Fl_Window second(320,180,"Independent root");int secondCloses=0;
  second.end();second.callback(counted,&secondCloses);second.show();shortcut(&second);
  check(secondCloses==0&&second.shown(),"Independent top-level application window also stays open");second.hide();
  Fl_Window dialog(280,160,"Non-modal dialog");int dialogCloses=0;
  dialog.end();dialog.set_non_modal();dialog.callback(counted,&dialogCloses);
  Fl::first_window(&root);dialog.show();shortcut(&dialog);
  check(dialogCloses==1,"Escape invokes spawned non-modal dialog dismissal");dialog.hide();
  Fl_Window modal(280,160,"Modal dialog");int modalCloses=0;
  modal.end();modal.set_modal();modal.callback(counted,&modalCloses);
  Fl::first_window(&root);modal.show();shortcut(&modal);
  check(modalCloses==1,"Escape invokes modal dialog dismissal");
  shortcut(nullptr);check(modalCloses==2,"Active modal dialog still handles Escape without a window argument");modal.hide();
  root.cancelAction=true;shortcut(&root);
  check(root.cancelled==1&&rootCloses==0,"Explicit application/widget Escape actions continue to work");root.cancelAction=false;
  shortcut(&root,'q',FL_CTRL);check(root.quit==1,"Explicit Quit shortcut continues to work");
  shortcut(nullptr);check(rootCloses==0,"Stale Escape without a window is harmless");
  root.do_callback();check(rootCloses==1,"Explicit titlebar/close callback remains available");
  std::printf("%d Escape dispatch checks passed\n",checks);
}
