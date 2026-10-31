// SPDX-License-Identifier: MIT
#pragma once
extern "C" {
#include <libavcodec/avcodec.h>
#include <libavcodec/bsf.h>
#include <libavfilter/avfilter.h>
#include <libavformat/avformat.h>
#include <libavutil/audio_fifo.h>
#include <libavutil/hwcontext.h>
}
#include <atomic>
#include <functional>
#include <memory>
#include <mutex>
#include <nlohmann/json.hpp>
#include <string>
#include <thread>
namespace capture {
using Json = nlohmann::json;
struct FrameDelete {
  void operator()(AVFrame *p) const { av_frame_free(&p); }
};
using Frame = std::unique_ptr<AVFrame, FrameDelete>;
inline Frame frame() {
  auto f = Frame(av_frame_alloc());
  if (!f)
    throw std::bad_alloc();
  return f;
}
uint64_t nowNs();
void check(int status, const std::string &operation);
std::string defaultDevice();
struct Options {
  std::string source = "desktop", encoder = "auto", device = defaultDevice();
  std::string videoCodec = "h264", audio = "off";
  std::string output, server = "http://127.0.0.1:49640", report;
  int fps = 30, width = 1920, bitrate = 12000000, eye = 0;
  uint32_t pid = 0;
  double seconds = 0;
  void validate() const;
};
std::string videoEncoderName(const std::string &backend,
                             const std::string &codec);
struct AudioBlock {
  uint64_t timestamp = 0;
  std::vector<float> samples;
};
class AudioInput {
  struct Impl;
  std::unique_ptr<Impl> impl;

public:
  AudioInput(const std::string &mode, std::atomic<bool> &stop);
  ~AudioInput();
  std::vector<AudioBlock> drain();
  uint64_t droppedSamples() const;
};
class Encoder {
  AVCodecContext *codec = nullptr;
  AVCodecContext *audioCodec = nullptr;
  AVBSFContext *videoBsf = nullptr;
  AVAudioFifo *audioFifo = nullptr;
  AVFormatContext *mux = nullptr;
  AVFilterGraph *graph = nullptr;
  AVFilterContext *input = nullptr, *sink = nullptr;
  AVBufferRef *device = nullptr;
  AVIOContext *io = nullptr;
  int fd = -1;
  bool header = false, finished = false;
  int inWidth = 0, inHeight = 0, inFormat = -1;
  int64_t previous = -1;
  void packets(AVCodecContext *, int streamIndex);
  void muxPacket(AVPacket *, AVRational timeBase, int streamIndex);
  void filteredPackets(AVPacket *);
  void audioFrames(bool final);

public:
  std::string backend;
  std::string inputFormat;
  std::string codecName;
  uint64_t audioSamples = 0;
  uint64_t frames = 0, bytes = 0;
  int width = 0, height = 0;
  ~Encoder();
  void open(const Options &, const AVFrame &first);
  void write(AVFrame &, int64_t ptsUs);
  void writeAudio(AudioBlock block, uint64_t epoch);
  void finish();
};
// Every source returns only a current frame; no unbounded delivery queue.
class Source {
public:
  virtual ~Source() = default;
  virtual Frame next() = 0;
  virtual std::string transport() const = 0;
};
std::unique_ptr<Source> makeSource(const Options &, std::atomic<bool> &stop);
std::unique_ptr<Source> makeEyeSource(const Options &);
Frame testFrame(int width = 320, int height = 240);
// Selective mmap reader, wire-compatible with the OBS fork. No full SBS copy.
Frame readEye(const std::string &path, uint64_t &sequence, int eye,
              uint64_t *timestamp = nullptr);
class Job {
  std::thread worker;
  std::atomic<bool> stopped{false};
  mutable std::mutex mutex;
  Json state = Json::object();

public:
  ~Job();
  void start(Options options);
  void stop() { stopped = true; }
  void join();
  Json snapshot() const;
};
} // namespace capture
