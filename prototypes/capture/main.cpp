// SPDX-License-Identifier: MIT
#include "../../native/tray/desktop-menu.h"
#include "capture.h"
#include "gpu-window.h"
#include <FL/Fl_Button.H>
#include <FL/Fl_Choice.H>
#include <FL/Fl_Input.H>
#include <FL/Fl_Native_File_Chooser.H>
#include <csignal>
#include <filesystem>
#include <gio/gio.h>
#include <iomanip>
#include <iostream>
#include <sstream>
namespace {
volatile sig_atomic_t interrupted = 0;
void signalStop(int) { interrupted = 1; }
std::string defaultOutput() {
  const char *videos = g_get_user_special_dir(G_USER_DIRECTORY_VIDEOS);
  auto directory = std::filesystem::path(videos ? videos : g_get_home_dir());
  std::time_t time = std::time(nullptr);
  std::tm local{};
  localtime_r(&time, &local);
  std::ostringstream name;
  name << "DomeD-" << std::put_time(&local, "%Y%m%d-%H%M%S") << ".mkv";
  return (directory / name.str()).string();
}
class App {
  capture::Job job;
  dx::GpuWindow window{780, 660, "DomeD Capture — prototype"};
  Fl_Menu_Bar menu{0, 0, 0, 0};
  std::unique_ptr<DesktopMenu> desktopMenu;
  Fl_Choice source{185, 28, 560, 34, "Source"};
  Fl_Choice encoder{185, 80, 560, 34, "Encoder"};
  Fl_Choice videoCodec{185, 132, 240, 34, "Video codec"};
  Fl_Choice audio{555, 132, 190, 34, "Audio"};
  Fl_Input device{185, 184, 560, 34, "GPU render node"};
  Fl_Choice width{185, 236, 240, 34, "Maximum width"};
  Fl_Choice fps{590, 236, 155, 34, "Frame limit"};
  Fl_Input pid{185, 288, 240, 34, "OpenXR PID"};
  Fl_Input destination{185, 340, 465, 34, "Recording"};
  Fl_Button browse{660, 340, 85, 34, "Browse"};
  Fl_Box note{28, 392, 720, 60,
              "DomeD XR records its surfaces from one eye. Runtime "
              "overlays,\npassthrough and the procedural backdrop are "
              "excluded. Audio is optional."};
  Fl_Box status{28, 466, 720, 100,
                "Ready — automatic encoding requires working GPU hardware."};
  Fl_Button start{28, 604, 160, 36, "Start recording"};
  Fl_Button stop{200, 604, 140, 36, "Stop and save"};
  Fl_Button close{608, 604, 140, 36, "Close"};
  bool closing = false;
  void codecChoices() {
    capture::Options o;
    o.encoder =
        std::array<const char *, 5>{"auto", "vaapi", "qsv", "nvenc", "software"}
            .at(encoder.value());
    o.device = device.value();
    auto backend =
        o.encoder == "auto"
            ? (access(o.device.c_str(), R_OK | W_OK) == 0 ? "vaapi" : "nvenc")
            : o.encoder;
    for (int i = 0; i < 4; ++i) {
      auto name = capture::videoEncoderName(
          backend,
          std::array<const char *, 4>{"h264", "h265", "vp9", "av1"}.at(i));
      videoCodec.mode(
          i, avcodec_find_encoder_by_name(name.c_str()) ? 0 : FL_MENU_INACTIVE);
    }
    if (videoCodec.menu()[videoCodec.value()].flags & FL_MENU_INACTIVE)
      for (int i = 0; i < 4; ++i)
        if (!(videoCodec.menu()[i].flags & FL_MENU_INACTIVE)) {
          videoCodec.value(i);
          break;
        }
  }
  static void tick(void *data) {
    auto &s = *static_cast<App *>(data);
    dx::appearance();
    auto state = s.job.snapshot();
    bool running = state.value("running", false);
    std::ostringstream text;
    text << state.value("status", std::string("Ready"));
    if (state.contains("encoder"))
      text << " · " << state["encoder"].get<std::string>();
    if (state.contains("frames"))
      text << " · " << state["frames"] << " frames";
    if (state.contains("seconds"))
      text << " · " << int(state["seconds"].get<double>()) << " s";
    if (state.contains("error"))
      text << "\n" << state["error"].get<std::string>();
    else if (state.contains("transport"))
      text << "\n" << state["transport"].get<std::string>();
    s.status.copy_label(text.str().c_str());
    for (auto w : std::array<Fl_Widget *, 12>{
             &s.source, &s.encoder, &s.device, &s.width, &s.fps, &s.pid,
             &s.destination, &s.browse, &s.start, &s.close, &s.videoCodec,
             &s.audio}) {
      if (running && w != &s.close)
        w->deactivate();
      else
        w->activate();
    }
    if (running)
      s.stop.activate();
    else
      s.stop.deactivate();
    if (s.closing && !running) {
      s.job.join();
      s.window.hide();
      return;
    }
    if (s.window.shown())
      Fl::repeat_timeout(.2, tick, data);
  }
  void quit() {
    closing = true;
    job.stop();
    if (!job.snapshot().value("running", false))
      window.hide();
  }
  void begin() {
    try {
      capture::Options o;
      int selected = source.value();
      o.source = selected == 0 ? "desktop" : selected < 3 ? "xr" : "mirror";
      o.eye = (selected == 2 || selected == 4) ? 1 : 0;
      o.encoder = std::array<const char *, 5>{"auto", "vaapi", "qsv", "nvenc",
                                              "software"}
                      .at(encoder.value());
      o.device = device.value();
      o.videoCodec =
          std::array<const char *, 4>{"h264", "h265", "vp9", "av1"}.at(
              videoCodec.value());
      o.audio = std::array<const char *, 3>{"off", "system", "microphone"}.at(
          audio.value());
      o.width = std::array<int, 4>{1280, 1920, 2560, 3840}.at(width.value());
      o.fps = fps.value() == 0 ? 30 : 60;
      std::string process = pid.value();
      if (process.empty() ||
          process.find_first_not_of("0123456789") != std::string::npos ||
          std::stoull(process) > UINT32_MAX)
        throw std::runtime_error(
            "OpenXR PID must be zero or a valid process ID");
      o.pid = std::stoul(process);
      o.output = destination.value();
      o.report = o.output + ".json";
      job.start(o);
    } catch (const std::exception &e) {
      fl_alert("%s", e.what());
    }
  }

public:
  App() {
    menu.hide();
    menu.add(
        "&Recording/&Start", FL_CTRL + 'r',
        +[](Fl_Widget *, void *p) { static_cast<App *>(p)->begin(); }, this);
    menu.add(
        "&Recording/S&top and save", FL_CTRL + '.',
        +[](Fl_Widget *, void *p) { static_cast<App *>(p)->job.stop(); }, this);
    menu.add(
        "&Recording/&Quit", FL_META + 'q',
        +[](Fl_Widget *, void *p) { static_cast<App *>(p)->quit(); }, this);
    source.add("Desktop monitor (portal)|DomeD XR — left eye|DomeD XR — right "
               "eye|OpenXR mirror — left eye|OpenXR mirror — right eye");
    source.value(0);
    encoder.add("Automatic hardware|VA-API (Intel, AMD)|Intel QSV (Quick "
                "Sync)|NVENC (CPU "
                "input)|Software (explicit opt-in)");
    encoder.value(0);
    device.value(capture::defaultDevice().c_str());
    videoCodec.add("H.264|H.265 (HEVC)|VP9|AV1");
    videoCodec.value(0);
    videoCodec.tooltip("Choices depend on FFmpeg's available encoders. GPU "
                       "support is checked before the output file is created.");
    audio.add("Off|System output|Microphone");
    audio.value(0);
    audio.tooltip("Record the default PipeWire output monitor or microphone as "
                  "stereo Opus. Audio starts only when recording starts.");
    encoder.callback(
        +[](Fl_Widget *, void *data) {
          static_cast<App *>(data)->codecChoices();
        },
        this);
    device.when(FL_WHEN_CHANGED);
    device.callback(
        +[](Fl_Widget *, void *data) {
          static_cast<App *>(data)->codecChoices();
        },
        this);
    codecChoices();
    device.tooltip("Choose the same GPU as DomeD for direct XR capture. Use "
                   "VA-API or Intel QSV.");
    width.add("1280 px|1920 px|2560 px|3840 px");
    width.value(1);
    fps.add("30 fps|60 fps");
    fps.value(0);
    fps.tooltip("DomeD's current portal producer is capped at 30 fps.");
    pid.value("0");
    pid.tooltip("0 selects the newest OBS mirror producer, then pins it for "
                "this recording.");
    destination.value(defaultOutput().c_str());
    note.align(FL_ALIGN_LEFT | FL_ALIGN_INSIDE);
    status.align(FL_ALIGN_LEFT | FL_ALIGN_INSIDE | FL_ALIGN_WRAP);
    browse.callback(
        +[](Fl_Widget *, void *p) {
          auto &s = *static_cast<App *>(p);
          Fl_Native_File_Chooser chooser;
          chooser.title("Save recording");
          chooser.type(Fl_Native_File_Chooser::BROWSE_SAVE_FILE);
          chooser.filter("Matroska video\t*.mkv");
          chooser.preset_file(s.destination.value());
          if (chooser.show() == 0 && chooser.filename())
            s.destination.value(chooser.filename());
        },
        this);
    start.callback(
        +[](Fl_Widget *, void *p) { static_cast<App *>(p)->begin(); }, this);
    stop.callback(
        +[](Fl_Widget *, void *p) { static_cast<App *>(p)->job.stop(); }, this);
    close.callback(
        +[](Fl_Widget *, void *p) { static_cast<App *>(p)->quit(); }, this);
    window.callback(
        +[](Fl_Widget *, void *p) { static_cast<App *>(p)->quit(); }, this);
    window.end();
    window.xclass("dome-capture");
    dx::style(&window);
    window.size_range(780, 660, 780, 660);
  }
  int run() {
    window.show();
    desktopMenu = std::make_unique<DesktopMenu>(window, menu);
    Fl::add_timeout(.1, tick, this);
    return Fl::run();
  }
};
} // namespace
int main(int argc, char **argv) try {
  capture::Options o;
  o.output = defaultOutput();
  bool cli = false;
  for (int i = 1; i < argc; ++i) {
    std::string arg = argv[i];
    auto value = [&]() {
      if (++i >= argc)
        throw std::runtime_error("Missing value for " + arg);
      return std::string(argv[i]);
    };
    if (arg == "--help") {
      std::cout
          << "dome-capture [--record --source desktop|xr|mirror|test "
             "--eye left|right]\n"
             "  --output NEW.mkv --encoder auto|vaapi|qsv|nvenc|software\n"
             "  --codec h264|h265|vp9|av1 --audio off|system|microphone\n"
             "  --device /dev/dri/renderD129 --width 1920 --fps 30 "
             "--bitrate 12000000\n"
             "  --pid 0 --seconds 0 --server http://127.0.0.1:49640 "
             "--report NEW.json\n"
             "No arguments opens FLTK. Stop CLI recording with "
             "SIGINT/SIGTERM.\n";
      return 0;
    } else if (arg == "--record")
      cli = true;
    else if (arg == "--source")
      o.source = value();
    else if (arg == "--eye") {
      auto eye = value();
      if (eye != "left" && eye != "right")
        throw std::runtime_error("Eye must be left or right");
      o.eye = eye == "right";
    } else if (arg == "--output")
      o.output = value();
    else if (arg == "--encoder")
      o.encoder = value();
    else if (arg == "--codec")
      o.videoCodec = value();
    else if (arg == "--audio")
      o.audio = value();
    else if (arg == "--device")
      o.device = value();
    else if (arg == "--width")
      o.width = std::stoi(value());
    else if (arg == "--fps")
      o.fps = std::stoi(value());
    else if (arg == "--bitrate")
      o.bitrate = std::stoi(value());
    else if (arg == "--pid") {
      auto pid = std::stoull(value());
      if (pid > UINT32_MAX)
        throw std::runtime_error("Invalid PID");
      o.pid = pid;
    } else if (arg == "--seconds")
      o.seconds = std::stod(value());
    else if (arg == "--server")
      o.server = value();
    else if (arg == "--report")
      o.report = value();
    else
      throw std::runtime_error("Unknown argument: " + arg);
  }
  if (cli) {
    signal(SIGINT, signalStop);
    signal(SIGTERM, signalStop);
    capture::Job job;
    job.start(o);
    do {
      if (interrupted)
        job.stop();
      std::this_thread::sleep_for(std::chrono::milliseconds(50));
    } while (job.snapshot().value("running", false));
    job.join();
    auto result = job.snapshot();
    std::cout << result.dump(2) << '\n';
    return result.contains("error") || result.contains("finalize_error") ||
                   result.contains("report_error")
               ? 1
               : 0;
  }
  Fl::lock();
  dx::init();
  App app;
  return app.run();
} catch (const std::exception &e) {
  std::cerr << "dome-capture: " << e.what() << '\n';
  return 1;
}
