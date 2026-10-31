// SPDX-License-Identifier: MIT
#include "capture.h"
#include <condition_variable>
#include <cstring>
#include <deque>
#include <pipewire/pipewire.h>
#include <spa/buffer/meta.h>
#include <spa/param/audio/format-utils.h>
namespace capture {
struct AudioInput::Impl {
  pw_thread_loop *loop = nullptr;
  pw_stream *stream = nullptr;
  spa_hook hook{};
  mutable std::mutex mutex;
  std::condition_variable ready;
  std::deque<AudioBlock> blocks;
  std::string error;
  bool received = false, streaming = false;
  size_t queued = 0;
  uint64_t dropped = 0, last = nowNs();
  void process() noexcept {
    pw_buffer *buffer;
    while ((buffer = pw_stream_dequeue_buffer(stream))) {
      try {
        auto b = buffer->buffer;
        if (b->n_datas != 1)
          throw std::runtime_error("Unsupported PipeWire audio layout");
        auto &d = b->datas[0];
        auto c = d.chunk;
        if (!d.data || !c || !c->size ||
            (c->flags & SPA_CHUNK_FLAG_CORRUPTED)) {
          pw_stream_queue_buffer(stream, buffer);
          continue;
        }
        if (c->size % (2 * sizeof(float)) ||
            uint64_t(c->offset) + c->size > d.maxsize ||
            c->size > 48000 * 2 * sizeof(float))
          throw std::runtime_error("Invalid PipeWire audio buffer");
        AudioBlock block;
        size_t frames = c->size / (2 * sizeof(float));
        block.timestamp = nowNs() - frames * 1000000000ull / 48000;
        auto header = static_cast<spa_meta_header *>(spa_buffer_find_meta_data(
            b, SPA_META_Header, sizeof(spa_meta_header)));
        auto now = nowNs();
        if (header && header->pts > 0 && uint64_t(header->pts) <= now &&
            now - uint64_t(header->pts) < 2000000000ull)
          block.timestamp = header->pts;
        block.samples.resize(frames * 2);
        memcpy(block.samples.data(), static_cast<uint8_t *>(d.data) + c->offset,
               c->size);
        std::lock_guard<std::mutex> lock(mutex);
        // Bound capture to 250 ms. Timestamp gaps remain visible to the muxer
        // and become silence, never compressed time or a drifting audio clock.
        while (!blocks.empty() && queued + frames > 12000) {
          auto n = blocks.front().samples.size() / 2;
          queued -= n;
          dropped += n;
          blocks.pop_front();
        }
        if (frames <= 12000) {
          queued += frames;
          blocks.push_back(std::move(block));
        } else
          dropped += frames;
        last = now;
        received = true;
        ready.notify_all();
      } catch (const std::exception &e) {
        std::lock_guard<std::mutex> lock(mutex);
        error = e.what();
        ready.notify_all();
      }
      pw_stream_queue_buffer(stream, buffer);
    }
  }
  void open(const std::string &mode, std::atomic<bool> &stop) {
    pw_init(nullptr, nullptr);
    loop = pw_thread_loop_new("dome-capture-audio", nullptr);
    if (!loop)
      throw std::runtime_error("Create PipeWire audio loop");
    auto properties = pw_properties_new(
        PW_KEY_MEDIA_TYPE, "Audio", PW_KEY_MEDIA_CATEGORY, "Capture",
        PW_KEY_MEDIA_ROLE, "Production", PW_KEY_NODE_NAME, "dome-capture-audio",
        PW_KEY_STREAM_CAPTURE_SINK, mode == "system" ? "true" : "false",
        nullptr);
    static const pw_stream_events events = [] {
      pw_stream_events e{};
      e.version = PW_VERSION_STREAM_EVENTS;
      e.state_changed = +[](void *data, pw_stream_state, pw_stream_state state,
                            const char *reason) {
        auto &s = *static_cast<Impl *>(data);
        std::lock_guard<std::mutex> lock(s.mutex);
        if (state == PW_STREAM_STATE_STREAMING)
          s.streaming = true;
        if (state == PW_STREAM_STATE_ERROR ||
            (state == PW_STREAM_STATE_UNCONNECTED && s.streaming))
          s.error = reason ? reason : "Audio device disconnected";
        s.ready.notify_all();
      };
      e.param_changed = +[](void *data, uint32_t id, const spa_pod *pod) {
        if (id != SPA_PARAM_Format || !pod)
          return;
        spa_audio_info_raw format{};
        if (spa_format_audio_raw_parse(pod, &format) < 0 ||
            format.format != SPA_AUDIO_FORMAT_F32 || format.rate != 48000 ||
            format.channels != 2) {
          auto &s = *static_cast<Impl *>(data);
          std::lock_guard<std::mutex> lock(s.mutex);
          s.error = "Audio device did not negotiate stereo 48 kHz float audio";
          s.ready.notify_all();
        }
      };
      e.process = +[](void *data) { static_cast<Impl *>(data)->process(); };
      return e;
    }();
    stream =
        pw_stream_new_simple(pw_thread_loop_get_loop(loop),
                             "DomeD Capture audio", properties, &events, this);
    if (!stream)
      throw std::runtime_error("Create PipeWire audio capture stream");
    uint8_t storage[1024];
    spa_pod_builder builder = SPA_POD_BUILDER_INIT(storage, sizeof(storage));
    spa_audio_info_raw format{};
    format.format = SPA_AUDIO_FORMAT_F32;
    format.rate = 48000;
    format.channels = 2;
    format.position[0] = SPA_AUDIO_CHANNEL_FL;
    format.position[1] = SPA_AUDIO_CHANNEL_FR;
    const spa_pod *params[] = {
        spa_format_audio_raw_build(&builder, SPA_PARAM_EnumFormat, &format)};
    check(pw_stream_connect(
              stream, PW_DIRECTION_INPUT, PW_ID_ANY,
              static_cast<pw_stream_flags>(PW_STREAM_FLAG_AUTOCONNECT |
                                           PW_STREAM_FLAG_MAP_BUFFERS |
                                           PW_STREAM_FLAG_DONT_RECONNECT),
              params, 1),
          "Connect PipeWire audio");
    check(pw_thread_loop_start(loop), "Start PipeWire audio loop");
    std::unique_lock<std::mutex> lock(mutex);
    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (!received && error.empty() && !stop &&
           std::chrono::steady_clock::now() < deadline)
      ready.wait_for(lock, std::chrono::milliseconds(50));
    if (stop)
      throw std::runtime_error("Audio capture cancelled");
    if (!received && error.empty())
      throw std::runtime_error("No audio from the selected default device; "
                               "check PipeWire and WirePlumber");
    if (!error.empty())
      throw std::runtime_error(error);
  }
  ~Impl() {
    if (loop)
      pw_thread_loop_stop(loop);
    if (stream)
      pw_stream_destroy(stream);
    if (loop)
      pw_thread_loop_destroy(loop);
  }
};
AudioInput::AudioInput(const std::string &mode, std::atomic<bool> &stop)
    : impl(std::make_unique<Impl>()) {
  impl->open(mode, stop);
}
AudioInput::~AudioInput() = default;
std::vector<AudioBlock> AudioInput::drain() {
  std::lock_guard<std::mutex> lock(impl->mutex);
  if (!impl->error.empty())
    throw std::runtime_error(impl->error);
  if (nowNs() - impl->last > 2000000000ull)
    throw std::runtime_error("Audio capture stopped delivering samples");
  std::vector<AudioBlock> result;
  while (!impl->blocks.empty()) {
    result.push_back(std::move(impl->blocks.front()));
    impl->blocks.pop_front();
  }
  impl->queued = 0;
  return result;
}
uint64_t AudioInput::droppedSamples() const {
  std::lock_guard<std::mutex> lock(impl->mutex);
  return impl->dropped;
}
} // namespace capture
