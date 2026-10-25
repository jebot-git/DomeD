// Native FLTK Wayland menus. No DomeD-specific fixture placement or input logic.
#include <FL/Fl.H>
#include <FL/Fl_Window.H>
#include <FL/Fl_Menu_Bar.H>
#include <FL/Fl_Menu_Button.H>
#include <FL/Fl_Box.H>
#include <FL/Fl_Choice.H>
#include <cstdio>
#include <string>

static Fl_Window *window;
static int sequence;
static void action(Fl_Widget *, void *value) {
    std::string title = "FLTK " + std::string(static_cast<const char *>(value)) +
        " " + std::to_string(++sequence);
    window->copy_label(title.c_str());
    std::printf("ACTION %s\n", title.c_str()); std::fflush(stdout);
}
template<class Menu> static void items(Menu &menu, const char *prefix) {
    std::string p(prefix);
    menu.add((p+"Activate").c_str(), 0, action, (void *)"activate");
    menu.add((p+"Nested/Nested action").c_str(), 0, action, (void *)"nested");
    menu.add((p+"Disabled").c_str(), 0, action, (void *)"disabled", FL_MENU_INACTIVE);
    menu.add((p+"Toggle").c_str(), 0, action, (void *)"toggle", FL_MENU_TOGGLE);
}
int main(int argc, char **argv) {
    Fl_Window top(480,320,"FLTK fixture"); window=&top;
    top.xclass("org.domed.FltkPopupFixture");
    Fl_Menu_Bar bar(0,0,480,26); items(bar,"File/");
    Fl_Menu_Button menu(40,60,180,36,"Open menu"); items(menu,"");
    Fl_Choice choice(260,60,180,36,"Character"); items(choice,"");
    Fl_Choice lowerChoice(260,250,180,36,"Lower rollout"); items(lowerChoice,"");
    Fl_Box info(30,150,420,70,"Native FLTK menus: activate, nested, disabled, toggle");
    top.end(); top.show(argc,argv);
    return Fl::run();
}
