// SPDX-License-Identifier: GPL-2.0-or-later
// Dome integration for the upstream Pinentry FLTK frontend.
#pragma once
#include "fltk_appearance.h"
#include <cerrno>
#include <csignal>
#include <cstring>
#include <sys/resource.h>

namespace dome_pinentry {
inline bool sudo_mode = false;

class Window : public Fl_Window {
public:
  Window(int width, int height, const char *title)
      : Fl_Window(width, height, title) {
    xclass(sudo_mode ? "org.domed.Askpass" : "org.domed.Pinentry");
  }
  int handle(int event) override {
    if (event == FL_KEYDOWN && Fl::event_key() == 'w' &&
        (Fl::event_state() & (FL_CTRL | FL_ALT | FL_META)) == FL_CTRL) {
      do_callback();
      return 1;
    }
    return Fl_Window::handle(event);
  }
};

inline bool write_all(const char *data, size_t count) {
  while (count) {
    ssize_t written = write(STDOUT_FILENO, data, count);
    if (written < 0 && errno == EINTR) continue;
    if (written <= 0) return false;
    data += written;
    count -= written;
  }
  return true;
}

// Called after pinentry_init(), which initializes secure memory and drops
// privileges. The normal Assuan frontend remains available without --askpass.
inline int askpass(int argc, char **argv, pinentry_cmd_handler_t handler) {
  if (argc > 3) {
    std::fputs("Usage: dome-askpass [sudo prompt]\n", stderr);
    return 2;
  }
  if ((!getenv("WAYLAND_DISPLAY") || !*getenv("WAYLAND_DISPLAY")) &&
      (!getenv("DISPLAY") || !*getenv("DISPLAY"))) {
    std::fputs("Dome askpass requires a graphical display\n", stderr);
    return 1;
  }
  // No Assuan transcript, shell, password cache or temporary password file.
  struct rlimit limit = {0, 0};
  setrlimit(RLIMIT_CORE, &limit);
  std::signal(SIGPIPE, SIG_IGN);
  struct pinentry request {};
  request.title = const_cast<char *>("DomeD — sudo authentication");
  request.description = argc == 3 ? argv[2] : const_cast<char *>("sudo requests administrator authentication.");
  request.prompt = const_cast<char *>("Password:");
  request.ok = const_cast<char *>("_Authenticate");
  request.cancel = const_cast<char *>("_Cancel");
  if (!pinentry_setbufferlen(&request, 2048)) return 1;
  int result = handler(&request);
  bool good = result >= 0 && request.pin && result < request.pin_len;
  if (good) {
    // sudo's askpass protocol is exactly one line, including for an empty PIN.
    for (int i = 0; i < result; ++i)
      if (!request.pin[i] || request.pin[i] == '\r' || request.pin[i] == '\n') good = false;
    if (good) good = write_all(request.pin, result) && write_all("\n", 1);
  }
  if (request.pin) {
    explicit_bzero(request.pin, request.pin_len);
    secmem_free(request.pin);
  }
  return good ? 0 : 1;
}
} // namespace dome_pinentry
