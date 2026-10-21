// SPDX-License-Identifier: MIT
#include "fltk_appearance.h"
#include "gpu-window.h"
#include <FL/Fl.H>
#include <FL/Fl_Box.H>
#include <FL/Fl_Button.H>
#include <FL/Fl_Double_Window.H>
#include <FL/Fl_Multi_Browser.H>
#include <cstdio>
#include <nlohmann/json.hpp>
extern "C" int dome_portal_picker(int argc, char **argv) try {
  if (argc != 2)
    return 2;
  auto config = nlohmann::json::parse(argv[1]);
  dx::init();
  dx::GpuWindow dialog(600, 360, "Share your screen — DomeD");
  Fl_Box label(20, 10, 560, 70);
  auto app = config.value("app_id", std::string("An application"));
  label.copy_label(
      (app + " wants to share your screen.\nChoose the monitors it may see.")
          .c_str());
  label.align(FL_ALIGN_LEFT | FL_ALIGN_INSIDE | FL_ALIGN_WRAP);
  Fl_Multi_Browser list(20, 90, 560, 200);
  if (!config.value("multiple", false))
    list.type(FL_HOLD_BROWSER);
  list.format_char(0);
  auto outputs = config.value("outputs", nlohmann::json::array());
  for (auto &output : outputs) {
    auto text = output.value("name", std::string()) + " — " +
                std::to_string(output.value("width", 0)) + " × " +
                std::to_string(output.value("height", 0));
    list.add(text.c_str());
  }
  Fl_Button cancel(300, 310, 130, 32, "Cancel"),
      share(450, 310, 130, 32, "Share");
  bool accepted = false;
  struct State {
    bool *accepted;
    Fl_Multi_Browser *list;
    Fl_Window *dialog;
  };
  State state{&accepted, &list, &dialog};
  share.callback(
      [](Fl_Widget *, void *p) {
        auto s = (State *)p;
        for (int i = 1; i <= s->list->size(); ++i)
          if (s->list->selected(i)) {
            *s->accepted = true;
            s->dialog->hide();
            return;
          }
      },
      &state);
  cancel.callback([](Fl_Widget *, void *p) { ((Fl_Window *)p)->hide(); },
                  &dialog);
  dialog.end();
  dialog.xclass("org.domed.ScreenShare");
  dialog.set_modal();
  dx::style(&dialog);
  dialog.show();
  while (dialog.shown())
    Fl::wait();
  if (!accepted)
    return 1;
  nlohmann::json selected = nlohmann::json::array();
  for (int i = 1; i <= list.size(); ++i)
    if (list.selected(i))
      selected.push_back(outputs[i - 1]["name"]);
  auto data = selected.dump();
  printf("%s\n", data.c_str());
  fflush(stdout);
  dx::GpuWindow active(440, 140, "Screen sharing active — DomeD");
  Fl_Box text(20, 10, 400, 50, "Your selected monitors are being shared.");
  Fl_Button stop(120, 80, 200, 36, "Stop sharing");
  stop.callback([](Fl_Widget *, void *p) { ((Fl_Window *)p)->hide(); },
                &active);
  active.end();
  active.xclass("org.domed.ScreenShare");
  dx::style(&active);
  active.show();
  while (active.shown())
    Fl::wait();
  return 0;
} catch (...) {
  return 2;
}
