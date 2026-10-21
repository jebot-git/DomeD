// Native FLTK dialog fixture; shares the portal picker's session appearance.
#include "../native/fltk_appearance.h"
#include <FL/Fl_Native_File_Chooser.H>
int main() {
  dx::init();
  Fl_Window main(400, 200, "Chooser appearance fixture");
  main.xclass("org.domed.ChooserProbe");
  main.end();
  main.show();
  Fl::add_timeout(.3, [](void *) {
    Fl_Native_File_Chooser chooser;
    chooser.title("Choose File");
    int result = chooser.show();
    std::printf("CHOOSER_RESULT %d\n", result);
    std::fflush(stdout);
  });
  return Fl::run();
}
