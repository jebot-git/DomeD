// SPDX-License-Identifier: MIT
#include "capture.h"
#include "third_party/obsmirror/frame_ipc.h"
#include <algorithm>
#include <fcntl.h>
#include <filesystem>
#include <fstream>
#include <gio/gio.h>
#include <gio/gunixfdlist.h>
#include <pipewire/pipewire.h>
#include <spa/param/buffers.h>
#include <spa/param/video/format-utils.h>
#include <sys/file.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <unistd.h>
namespace capture {
Frame readEye(const std::string &path, uint64_t &sequence, int eye,
              uint64_t *timestamp) {
  int fd = open(path.c_str(), O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
  if (fd < 0)
    return {};
  struct Handle {
    int fd;
    void *map = MAP_FAILED;
    size_t size = 0;
    ~Handle() {
      if (map != MAP_FAILED)
        munmap(map, size);
      flock(fd, LOCK_UN);
      close(fd);
    }
  } handle{fd};
  struct stat st{};
  if (flock(fd, LOCK_SH | LOCK_NB) || fstat(fd, &st) || !S_ISREG(st.st_mode) ||
      st.st_uid != getuid() || (st.st_mode & 0077) ||
      st.st_size < off_t(sizeof(linux_mirror::FrameHeader)) ||
      st.st_size > off_t(sizeof(linux_mirror::FrameHeader) +
                         linux_mirror::kMaxFrameBytes))
    return {};
  handle.size = st.st_size;
  handle.map = mmap(nullptr, st.st_size, PROT_READ, MAP_SHARED, fd, 0);
  if (handle.map == MAP_FAILED)
    return {};
  linux_mirror::FrameHeader h;
  memcpy(&h, handle.map, sizeof(h));
  auto now = nowNs();
  if (h.magic != linux_mirror::kMagic || h.version != 1 || !h.width ||
      h.width > 16384 || !h.height || h.height > 8192 || !h.leftWidth ||
      uint64_t(h.leftWidth) + h.rightWidth != h.width ||
      h.stride != uint64_t(h.width) * 4 ||
      uint64_t(h.stride) * h.height + sizeof(h) != uint64_t(st.st_size) ||
      h.sequence == sequence || h.timestamp > now ||
      now - h.timestamp > 3000000000ull)
    return {};
  if (eye == 1 && !h.rightWidth)
    throw std::runtime_error("Selected producer has no right eye");
  auto f = frame();
  f->format = AV_PIX_FMT_RGBA;
  f->width = eye ? h.rightWidth : h.leftWidth;
  f->height = h.height;
  check(av_frame_get_buffer(f.get(), 32), "Allocate selected eye");
  const auto *pixels = static_cast<const uint8_t *>(handle.map) + sizeof(h) +
                       (eye ? h.leftWidth * 4 : 0);
  for (unsigned y = 0; y < h.height; ++y)
    memcpy(f->data[0] + y * f->linesize[0], pixels + size_t(y) * h.stride,
           f->width * 4);
  sequence = h.sequence;
  if (timestamp)
    *timestamp = h.timestamp;
  return f;
}
namespace {
class Mirror final : public Source {
  Options options;
  std::string path;
  uint64_t sequence = 0, lastScan = 0;

public:
  explicit Mirror(const Options &o) : options(o) {}
  Frame next() override {
    if (nowNs() - lastScan > 1000000000ull) {
      // Pin a producer once found. Never switch applications during recording.
      if (path.empty())
        path = linux_mirror::newestFrame(options.pid);
      lastScan = nowNs();
    }
    return path.empty() ? Frame{} : readEye(path, sequence, options.eye);
  }
  std::string transport() const override {
    return "OBS mirror mmap; selected-eye CPU copy + GPU upload";
  }
};
class Test final : public Source {
public:
  Frame next() override { return testFrame(); }
  std::string transport() const override { return "Synthetic test pattern"; }
};
class Portal final : public Source {
  GMainContext *context = g_main_context_new();
  GDBusConnection *bus = nullptr;
  guint responseSubscription = 0, closedSubscription = 0;
  std::string session, error;
  std::map<std::string, GVariant *> responses;
  std::atomic<bool> &stop;
  unsigned serial = 0;
  pw_main_loop *loop = nullptr;
  pw_context *pwContext = nullptr;
  pw_core *core = nullptr;
  pw_stream *stream = nullptr;
  spa_hook hook{};
  spa_video_info_raw format{};
  Frame latest;
  bool wasStreaming = false;
  GVariant *call(const char *path, const char *iface, const char *method,
                 GVariant *args) {
    GError *e = nullptr;
    auto result = g_dbus_connection_call_sync(
        bus, "org.freedesktop.portal.Desktop", path, iface, method, args,
        nullptr, G_DBUS_CALL_FLAGS_NONE, 10000, nullptr, &e);
    if (!result) {
      std::string message = e ? e->message : "D-Bus call failed";
      g_clear_error(&e);
      throw std::runtime_error(message);
    }
    return result;
  }
  static GVariant *
  options(std::initializer_list<std::pair<const char *, GVariant *>> values) {
    GVariantBuilder b;
    g_variant_builder_init(&b, G_VARIANT_TYPE_VARDICT);
    for (auto &p : values)
      g_variant_builder_add(&b, "{sv}", p.first, p.second);
    return g_variant_builder_end(&b);
  }
  GVariant *request(const char *method, GVariant *args) {
    auto reply = call("/org/freedesktop/portal/desktop",
                      "org.freedesktop.portal.ScreenCast", method, args);
    const char *handle;
    g_variant_get(reply, "(&o)", &handle);
    std::string path = handle;
    g_variant_unref(reply);
    auto deadline = nowNs() + 120000000000ull;
    while (!responses.count(path)) {
      while (g_main_context_iteration(context, FALSE)) {
      }
      if (stop || nowNs() > deadline) {
        auto result = call(path.c_str(), "org.freedesktop.portal.Request",
                           "Close", nullptr);
        g_variant_unref(result);
        throw std::runtime_error(stop ? "Capture cancelled"
                                      : "Screen sharing request timed out");
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(5));
    }
    auto value = responses.at(path);
    responses.erase(path);
    guint code;
    GVariant *dict;
    g_variant_get(value, "(u@a{sv})", &code, &dict);
    g_variant_unref(value);
    if (code) {
      g_variant_unref(dict);
      throw std::runtime_error(code == 1 ? "Screen sharing cancelled"
                                         : "Screen sharing denied");
    }
    return dict;
  }
  std::string token() {
    return "dome_capture_" + std::to_string(getpid()) + "_" +
           std::to_string(++serial);
  }
  void process() {
    pw_buffer *newest = nullptr;
    while (auto buffer = pw_stream_dequeue_buffer(stream)) {
      if (newest)
        pw_stream_queue_buffer(stream, newest);
      newest = buffer;
    }
    if (!newest)
      return;
    // This callback runs on the ordinary loop, never a realtime audio thread.
    try {
      auto b = newest->buffer;
      if (b->n_datas != 1 || !format.size.width || !format.size.height)
        throw std::runtime_error("Unsupported PipeWire buffer");
      auto &d = b->datas[0];
      auto c = d.chunk;
      uint64_t row = uint64_t(format.size.width) * 4;
      if (!d.data || !c || c->stride < int64_t(row) ||
          (c->flags & SPA_CHUNK_FLAG_CORRUPTED) ||
          uint64_t(c->offset) + uint64_t(c->stride) * (format.size.height - 1) +
                  row >
              d.maxsize ||
          c->size < uint64_t(c->stride) * (format.size.height - 1) + row) {
        // Empty/corrupted buffers are legal during negotiation; await a good
        // one.
        pw_stream_queue_buffer(stream, newest);
        return;
      }
      auto f = frame();
      f->width = format.size.width;
      f->height = format.size.height;
      f->format = (format.format == SPA_VIDEO_FORMAT_RGBA ||
                   format.format == SPA_VIDEO_FORMAT_RGBx)
                      ? AV_PIX_FMT_RGBA
                      : AV_PIX_FMT_BGRA;
      check(av_frame_get_buffer(f.get(), 32), "Copy PipeWire frame");
      for (int y = 0; y < f->height; ++y)
        memcpy(f->data[0] + y * f->linesize[0],
               static_cast<uint8_t *>(d.data) + c->offset + y * c->stride, row);
      latest = std::move(f);
    } catch (const std::exception &e) {
      error = e.what();
    }
    pw_stream_queue_buffer(stream, newest);
  }

public:
  explicit Portal(std::atomic<bool> &cancel) : stop(cancel) {
    g_main_context_push_thread_default(context);
  }
  void open() {
    GError *e = nullptr;
    bus = g_bus_get_sync(G_BUS_TYPE_SESSION, nullptr, &e);
    if (!bus) {
      std::string message = e ? e->message : "No session bus";
      g_clear_error(&e);
      throw std::runtime_error(message);
    }
    responseSubscription = g_dbus_connection_signal_subscribe(
        bus, "org.freedesktop.portal.Desktop", "org.freedesktop.portal.Request",
        "Response", nullptr, nullptr, G_DBUS_SIGNAL_FLAGS_NONE,
        +[](GDBusConnection *, const gchar *, const gchar *path, const gchar *,
            const gchar *, GVariant *args, gpointer data) {
          auto &s = *static_cast<Portal *>(data);
          if (s.responses.size() < 32) {
            auto found = s.responses.find(path);
            if (found != s.responses.end())
              g_variant_unref(found->second);
            s.responses[path] = g_variant_ref(args);
          }
        },
        this, nullptr);
    auto t = token();
    auto st = token();
    auto response =
        request("CreateSession",
                g_variant_new(
                    "(@a{sv})",
                    options({{"handle_token", g_variant_new_string(t.c_str())},
                             {"session_handle_token",
                              g_variant_new_string(st.c_str())}})));
    const char *sessionHandle = nullptr;
    if (!g_variant_lookup(response, "session_handle", "&s", &sessionHandle)) {
      // Some portal implementations return an object path variant.
      g_variant_lookup(response, "session_handle", "&o", &sessionHandle);
    }
    if (sessionHandle)
      session = sessionHandle;
    g_variant_unref(response);
    if (session.empty())
      throw std::runtime_error("Portal did not create a session");
    closedSubscription = g_dbus_connection_signal_subscribe(
        bus, "org.freedesktop.portal.Desktop", "org.freedesktop.portal.Session",
        "Closed", session.c_str(), nullptr, G_DBUS_SIGNAL_FLAGS_NONE,
        +[](GDBusConnection *, const gchar *, const gchar *, const gchar *,
            const gchar *, GVariant *, gpointer data) {
          static_cast<Portal *>(data)->error = "Screen sharing was revoked";
        },
        this, nullptr);
    t = token();
    response =
        request("SelectSources",
                g_variant_new(
                    "(o@a{sv})", session.c_str(),
                    options({{"handle_token", g_variant_new_string(t.c_str())},
                             {"types", g_variant_new_uint32(1)},
                             {"multiple", g_variant_new_boolean(FALSE)},
                             {"cursor_mode", g_variant_new_uint32(2)}})));
    g_variant_unref(response);
    t = token();
    response = request(
        "Start", g_variant_new("(os@a{sv})", session.c_str(), "",
                               options({{"handle_token",
                                         g_variant_new_string(t.c_str())}})));
    auto streams = g_variant_lookup_value(response, "streams",
                                          G_VARIANT_TYPE("a(ua{sv})"));
    if (!streams || g_variant_n_children(streams) != 1) {
      if (streams)
        g_variant_unref(streams);
      g_variant_unref(response);
      throw std::runtime_error("Select exactly one monitor");
    }
    auto selected = g_variant_get_child_value(streams, 0);
    guint node;
    GVariant *properties;
    g_variant_get(selected, "(u@a{sv})", &node, &properties);
    g_variant_unref(properties);
    g_variant_unref(selected);
    g_variant_unref(streams);
    g_variant_unref(response);
    GUnixFDList *fds = nullptr;
    response = g_dbus_connection_call_with_unix_fd_list_sync(
        bus, "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop", "org.freedesktop.portal.ScreenCast",
        "OpenPipeWireRemote",
        g_variant_new("(o@a{sv})", session.c_str(), options({})),
        G_VARIANT_TYPE("(h)"), G_DBUS_CALL_FLAGS_NONE, 10000, nullptr, &fds,
        nullptr, &e);
    if (!response) {
      std::string message = e ? e->message : "No authorized PipeWire remote";
      g_clear_error(&e);
      throw std::runtime_error(message);
    }
    gint index;
    g_variant_get(response, "(h)", &index);
    int remote = g_unix_fd_list_get(fds, index, &e);
    g_variant_unref(response);
    g_object_unref(fds);
    if (remote < 0) {
      g_clear_error(&e);
      throw std::runtime_error("Invalid PipeWire descriptor");
    }
    pw_init(nullptr, nullptr);
    loop = pw_main_loop_new(nullptr);
    if (!loop) {
      close(remote);
      throw std::runtime_error("Create PipeWire loop");
    }
    pwContext = pw_context_new(pw_main_loop_get_loop(loop), nullptr, 0);
    if (!pwContext) {
      close(remote);
      throw std::runtime_error("Create PipeWire context");
    }
    core = pw_context_connect_fd(pwContext, remote, nullptr, 0);
    if (!core)
      throw std::runtime_error("Connect authorized PipeWire remote");
    stream = pw_stream_new(
        core, "DomeD capture",
        pw_properties_new(PW_KEY_MEDIA_TYPE, "Video", PW_KEY_MEDIA_CATEGORY,
                          "Capture", PW_KEY_MEDIA_ROLE, "Screen", nullptr));
    if (!stream)
      throw std::runtime_error("Create PipeWire capture stream");
    static const pw_stream_events events = [] {
      pw_stream_events e{};
      e.version = PW_VERSION_STREAM_EVENTS;
      e.state_changed = +[](void *data, pw_stream_state, pw_stream_state state,
                            const char *err) {
        auto &s = *static_cast<Portal *>(data);
        if (state == PW_STREAM_STATE_STREAMING)
          s.wasStreaming = true;
        if (state == PW_STREAM_STATE_ERROR ||
            (state == PW_STREAM_STATE_UNCONNECTED && s.wasStreaming))
          s.error = err ? err : "Desktop stream disconnected";
      };
      e.param_changed = +[](void *data, uint32_t id, const spa_pod *pod) {
        auto &s = *static_cast<Portal *>(data);
        if (id == SPA_PARAM_Format && pod) {
          if (spa_format_video_raw_parse(pod, &s.format) < 0 ||
              !s.format.size.width || !s.format.size.height ||
              uint64_t(s.format.size.width) * s.format.size.height >
                  32 * 1024 * 1024)
            s.error = "Invalid PipeWire video format";
        }
      };
      e.process = +[](void *data) { static_cast<Portal *>(data)->process(); };
      return e;
    }();
    pw_stream_add_listener(stream, &hook, &events, this);
    uint8_t storage[2048];
    spa_pod_builder b = SPA_POD_BUILDER_INIT(storage, sizeof(storage));
    const spa_pod *params[] = {
        static_cast<spa_pod *>(spa_pod_builder_add_object(
            &b, SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat,
            SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video),
            SPA_FORMAT_mediaSubtype, SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
            SPA_FORMAT_VIDEO_format,
            SPA_POD_CHOICE_ENUM_Id(4, SPA_VIDEO_FORMAT_RGBA,
                                   SPA_VIDEO_FORMAT_BGRA, SPA_VIDEO_FORMAT_RGBx,
                                   SPA_VIDEO_FORMAT_BGRx)))};
    check(pw_stream_connect(
              stream, PW_DIRECTION_INPUT, node,
              static_cast<pw_stream_flags>(PW_STREAM_FLAG_AUTOCONNECT |
                                           PW_STREAM_FLAG_MAP_BUFFERS |
                                           PW_STREAM_FLAG_DONT_RECONNECT),
              params, 1),
          "Start desktop capture");
  }
  Frame next() override {
    while (g_main_context_iteration(context, FALSE)) {
    }
    pw_loop_iterate(pw_main_loop_get_loop(loop), 0);
    if (!error.empty())
      throw std::runtime_error(error);
    return std::move(latest);
  }
  std::string transport() const override {
    return "Portal PipeWire SHM + GPU upload";
  }
  ~Portal() override {
    if (stream)
      pw_stream_destroy(stream);
    if (core)
      pw_core_disconnect(core);
    if (pwContext)
      pw_context_destroy(pwContext);
    if (loop)
      pw_main_loop_destroy(loop);
    if (bus) {
      if (responseSubscription)
        g_dbus_connection_signal_unsubscribe(bus, responseSubscription);
      if (closedSubscription)
        g_dbus_connection_signal_unsubscribe(bus, closedSubscription);
      if (!session.empty()) {
        try {
          auto r = call(session.c_str(), "org.freedesktop.portal.Session",
                        "Close", nullptr);
          g_variant_unref(r);
        } catch (...) {
        }
      }
      g_object_unref(bus);
    }
    for (auto &p : responses)
      g_variant_unref(p.second);
    g_main_context_pop_thread_default(context);
    g_main_context_unref(context);
  }
};
} // namespace
std::unique_ptr<Source> makeSource(const Options &o, std::atomic<bool> &stop) {
  if (o.source == "mirror")
    return std::make_unique<Mirror>(o);
  if (o.source == "test")
    return std::make_unique<Test>();
  if (o.source == "xr")
    return makeEyeSource(o);
  auto portal = std::make_unique<Portal>(stop);
  portal->open();
  return portal;
}
Job::~Job() {
  stop();
  join();
}
void Job::join() {
  if (worker.joinable())
    worker.join();
}
Json Job::snapshot() const {
  std::lock_guard<std::mutex> lock(mutex);
  return state;
}
void Job::start(Options o) {
  o.validate();
  if (snapshot().value("running", false))
    throw std::runtime_error("Recording is already running");
  join();
  stopped = false;
  {
    std::lock_guard<std::mutex> lock(mutex);
    state = {{"running", true},
             {"status", "Preparing capture"},
             {"frames", 0},
             {"output", o.output}};
  }
  worker = std::thread([this, o] {
    Encoder encoder;
    std::unique_ptr<AudioInput> audio;
    Json result = {{"version", 1},
                   {"running", false},
                   {"source", o.source},
                   {"output", o.output},
                   {"eye", o.eye == 0 ? "left" : "right"}};
    uint64_t started = 0, lastFrame = nowNs(), deadline = nowNs();
    uint64_t missedDeadlines = 0, encodeNs = 0, maxEncodeNs = 0;
    rusage usageStart{};
    getrusage(RUSAGE_THREAD, &usageStart);
    try {
      auto source = makeSource(o, stopped);
      result["transport"] = source->transport();
      deadline = nowNs();
      lastFrame = deadline;
      while (!stopped) {
        auto f = source->next();
        auto now = nowNs();
        if (f) {
          if (!started) {
            if (o.audio != "off") {
              audio = std::make_unique<AudioInput>(o.audio, stopped);
              if (auto fresh = source->next())
                f = std::move(fresh);
            }
            encoder.open(o, *f);
            started = nowNs();
            lastFrame = started;
            result["input_format"] =
                f->format == AV_PIX_FMT_DRM_PRIME ? "drm_prime" : "cpu_rgb";
          }
          auto encodeStart = nowNs();
          encoder.write(*f, (nowNs() - started) / 1000);
          auto elapsed = nowNs() - encodeStart;
          encodeNs += elapsed;
          maxEncodeNs = std::max(maxEncodeNs, elapsed);
          lastFrame = nowNs();
          std::lock_guard<std::mutex> lock(mutex);
          state = {{"running", true},
                   {"status", "Recording"},
                   {"frames", encoder.frames},
                   {"bytes", encoder.bytes},
                   {"seconds", double(nowNs() - started) / 1e9},
                   {"encoder", encoder.backend},
                   {"transport", source->transport()},
                   {"output", o.output},
                   {"width", encoder.width},
                   {"height", encoder.height}};
        } else if (now - lastFrame > (started ? 3000000000ull : 10000000000ull))
          throw std::runtime_error(
              started ? "Source stopped delivering frames"
                      : "No capture frames; start a compatible source");
        if (audio && started) {
          for (auto &block : audio->drain())
            encoder.writeAudio(std::move(block), started);
        }
        if (started && o.seconds > 0 &&
            double(nowNs() - started) / 1e9 >= o.seconds)
          break;
        // Missed deadlines are skipped. Never encode a backlog or speed up
        // time.
        deadline += 1000000000ull / o.fps;
        if (deadline < nowNs()) {
          missedDeadlines += (nowNs() - deadline) / (1000000000ull / o.fps) + 1;
          deadline = nowNs();
        }
        while (!stopped && nowNs() < deadline)
          std::this_thread::sleep_for(std::chrono::milliseconds(2));
      }
      encoder.finish();
      result["status"] = started ? "Saved" : "Cancelled";
    } catch (const std::exception &e) {
      result["status"] = "Stopped";
      result["error"] = e.what();
      try {
        encoder.finish();
      } catch (const std::exception &finalize) {
        result["finalize_error"] = finalize.what();
      }
    }
    result["encoder"] = encoder.backend;
    result["video_codec"] = o.videoCodec;
    result["video_encoder"] = encoder.codecName;
    result["audio"] = o.audio;
    result["audio_samples"] = encoder.audioSamples;
    result["audio_dropped_samples"] = audio ? audio->droppedSamples() : 0;
    result["encoder_input_format"] = encoder.inputFormat;
    result["frames"] = encoder.frames;
    result["bytes"] = encoder.bytes;
    result["width"] = encoder.width;
    result["height"] = encoder.height;
    result["seconds"] = started ? double(nowNs() - started) / 1e9 : 0.;
    result["missed_deadlines"] = missedDeadlines;
    result["encode_ms_average"] =
        encoder.frames ? double(encodeNs) / encoder.frames / 1e6 : 0.;
    result["encode_ms_max"] = double(maxEncodeNs) / 1e6;
    rusage usageEnd{};
    getrusage(RUSAGE_THREAD, &usageEnd);
    auto cpu = [](const rusage &u) {
      return u.ru_utime.tv_sec + u.ru_stime.tv_sec +
             (u.ru_utime.tv_usec + u.ru_stime.tv_usec) / 1e6;
    };
    result["worker_cpu_seconds"] = cpu(usageEnd) - cpu(usageStart);
    if (!o.report.empty()) {
      int fd = open(o.report.c_str(),
                    O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
      if (fd < 0)
        result["report_error"] = strerror(errno);
      else {
        auto text = result.dump(2) + "\n";
        size_t offset = 0;
        while (offset < text.size()) {
          auto n = write(fd, text.data() + offset, text.size() - offset);
          if (n < 0 && errno == EINTR)
            continue;
          if (n <= 0) {
            result["report_error"] = strerror(errno);
            break;
          }
          offset += n;
        }
        close(fd);
      }
    }
    std::lock_guard<std::mutex> lock(mutex);
    state = std::move(result);
  });
}
} // namespace capture
