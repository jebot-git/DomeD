// SPDX-License-Identifier: MIT
#include "capture.h"
extern "C" {
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavutil/opt.h>
#include <libavutil/pixdesc.h>
}
#include <algorithm>
#include <chrono>
#include <cmath>
#include <fcntl.h>
#include <filesystem>
#include <unistd.h>
namespace capture {
std::string defaultDevice() {
  if (auto configured = getenv("DOME_CAPTURE_DEVICE");
      configured && *configured)
    return configured;
  std::vector<std::string> nodes;
  std::error_code error;
  for (const auto &entry :
       std::filesystem::directory_iterator("/dev/dri", error)) {
    auto name = entry.path().filename().string();
    if (name.rfind("renderD", 0) == 0 &&
        access(entry.path().c_str(), R_OK | W_OK) == 0)
      nodes.push_back(entry.path().string());
  }
  std::sort(nodes.begin(), nodes.end());
  return nodes.empty() ? "/dev/dri/renderD128" : nodes.back();
}
uint64_t nowNs() {
  return std::chrono::duration_cast<std::chrono::nanoseconds>(
             std::chrono::steady_clock::now().time_since_epoch())
      .count();
}
void check(int status, const std::string &operation) {
  if (status < 0) {
    char buffer[AV_ERROR_MAX_STRING_SIZE];
    av_strerror(status, buffer, sizeof(buffer));
    throw std::runtime_error(operation + ": " + buffer);
  }
}
void Options::validate() const {
  if (videoCodec != "h264" && videoCodec != "h265" && videoCodec != "vp9" &&
      videoCodec != "av1")
    throw std::runtime_error("Codec must be h264, h265, vp9 or av1");
  if (audio != "off" && audio != "system" && audio != "microphone")
    throw std::runtime_error("Audio must be off, system or microphone");
  if (source != "desktop" && source != "xr" && source != "mirror" &&
      source != "test")
    throw std::runtime_error("Source must be desktop, xr, mirror or test");
  if (encoder != "auto" && encoder != "vaapi" && encoder != "qsv" &&
      encoder != "nvenc" && encoder != "software")
    throw std::runtime_error(
        "Encoder must be auto, vaapi, qsv, nvenc or software");
  if (fps < 1 || fps > 60 || width < 64 || width > 3840 || width % 2 ||
      bitrate < 100000 || bitrate > 100000000 || eye < 0 || eye > 1 ||
      !std::isfinite(seconds) || seconds < 0 || seconds > 86400)
    throw std::runtime_error("Invalid capture limits (1–60 fps, even width "
                             "64–3840, 0.1–100 Mbps, eye 0/1)");
  if (source == "xr" && encoder != "auto" && encoder != "vaapi" &&
      encoder != "qsv")
    throw std::runtime_error(
        "DomeD GPU eye capture requires VA-API or Intel QSV");
  auto backend =
      encoder == "auto"
          ? (access(device.c_str(), R_OK | W_OK) == 0 ? "vaapi" : "nvenc")
          : encoder;
  auto implementation = videoEncoderName(backend, videoCodec);
  if (!avcodec_find_encoder_by_name(implementation.c_str()))
    throw std::runtime_error(
        "Selected codec is unavailable for this encoder: " + implementation);
  if (output.empty())
    throw std::runtime_error("Choose a new .mkv output file");
  if (output.size() < 4 || output.substr(output.size() - 4) != ".mkv")
    throw std::runtime_error("This prototype records Matroska (.mkv)");
  const std::string prefix = "http://127.0.0.1:";
  if (server.rfind(prefix, 0) != 0 || server.size() > prefix.size() + 5 ||
      server.size() == prefix.size() ||
      server.substr(prefix.size()).find_first_not_of("0123456789") !=
          std::string::npos ||
      std::stoi(server.substr(prefix.size())) < 1 ||
      std::stoi(server.substr(prefix.size())) > 65535)
    throw std::runtime_error("DomeD server must be http://127.0.0.1:PORT");
}
std::string videoEncoderName(const std::string &backend,
                             const std::string &codec) {
  std::string prefix = codec == "h265" ? "hevc" : codec;
  if (backend == "software") {
    if (codec == "h264")
      return "libx264";
    if (codec == "h265")
      return "libx265";
    if (codec == "vp9")
      return "libvpx-vp9";
    return "libsvtav1";
  }
  return prefix + "_" + backend;
}
namespace {
int writeBytes(void *data, const uint8_t *buffer, int size) {
  int fd = *static_cast<int *>(data), left = size;
  while (left) {
    auto n = ::write(fd, buffer, left);
    if (n < 0 && errno == EINTR)
      continue;
    if (n <= 0)
      return AVERROR(n ? errno : EIO);
    buffer += n;
    left -= n;
  }
  return size;
}
int64_t seekBytes(void *data, int64_t offset, int whence) {
  int fd = *static_cast<int *>(data);
  if (whence == AVSEEK_SIZE) {
    auto current = lseek(fd, 0, SEEK_CUR);
    auto end = lseek(fd, 0, SEEK_END);
    lseek(fd, current, SEEK_SET);
    return end;
  }
  auto result = lseek(fd, offset, whence & ~AVSEEK_FORCE);
  return result < 0 ? AVERROR(errno) : result;
}
} // namespace
Encoder::~Encoder() {
  // The worker calls finish explicitly, so write/trailer failures stay visible.
  avcodec_free_context(&codec);
  avcodec_free_context(&audioCodec);
  av_bsf_free(&videoBsf);
  if (audioFifo)
    av_audio_fifo_free(audioFifo);
  avfilter_graph_free(&graph);
  av_buffer_unref(&device);
  if (mux) {
    mux->pb = nullptr;
    avformat_free_context(mux);
  }
  if (io) {
    av_freep(&io->buffer);
    avio_context_free(&io);
  }
  if (fd >= 0)
    close(fd);
}
void Encoder::open(const Options &o, const AVFrame &first) {
  if (codec)
    throw std::runtime_error("Encoder already opened");
  inWidth = first.width;
  inHeight = first.height;
  inFormat = first.format;
  if (inWidth < 1 || inHeight < 1 ||
      uint64_t(inWidth) * inHeight > 32 * 1024 * 1024)
    throw std::runtime_error("Capture dimensions exceed 32 megapixels");
  double scale = std::min(1., double(o.width) / inWidth);
  width = std::max(2, int(inWidth * scale) / 2 * 2);
  height = std::max(2, int(inHeight * scale) / 2 * 2);
  backend = o.encoder;
  if (backend == "auto")
    backend = access(o.device.c_str(), R_OK | W_OK) == 0 ? "vaapi" : "nvenc";
  codecName = videoEncoderName(backend, o.videoCodec);
  const auto &name = codecName;
  auto implementation = avcodec_find_encoder_by_name(name.c_str());
  if (!implementation)
    throw std::runtime_error(
        name + " is unavailable; no software fallback is performed");
  graph = avfilter_graph_alloc();
  if (!graph)
    throw std::bad_alloc();
  graph->nb_threads = 1;
  input = avfilter_graph_alloc_filter(graph, avfilter_get_by_name("buffer"),
                                      "capture");
  if (!input)
    throw std::bad_alloc();
  auto parameters = av_buffersrc_parameters_alloc();
  if (!parameters)
    throw std::bad_alloc();
  parameters->format = first.format;
  parameters->width = first.width;
  parameters->height = first.height;
  parameters->time_base = {1, 1000000};
  parameters->frame_rate = {o.fps, 1};
  parameters->sample_aspect_ratio = {1, 1};
  parameters->color_space = AVCOL_SPC_RGB;
  parameters->color_range = AVCOL_RANGE_JPEG;
  parameters->hw_frames_ctx = first.hw_frames_ctx;
  int status = av_buffersrc_parameters_set(input, parameters);
  av_free(parameters);
  check(status, "Configure video input");
  check(avfilter_init_str(input, nullptr), "Initialize video input");
  std::vector<std::pair<std::string, std::string>> filters;
  if (backend == "vaapi" || backend == "qsv") {
    check(av_hwdevice_ctx_create(&device, AV_HWDEVICE_TYPE_VAAPI,
                                 o.device.c_str(), nullptr, 0),
          "Open VA-API device " + o.device);
    filters.push_back(
        first.format == AV_PIX_FMT_DRM_PRIME
            ? std::make_pair("hwmap", "derive_device=vaapi:mode=read+direct")
            : std::make_pair("hwupload", ""));
    filters.emplace_back(
        "scale_vaapi",
        "w=" + std::to_string(width) + ":h=" + std::to_string(height) +
            ":format=nv12:out_color_matrix=bt709:out_range=limited:out_color_"
            "primaries=bt709:out_color_transfer=bt709");
    if (backend == "qsv") {
      // QSV owns a small fixed surface pool and exposes those surfaces back to
      // VA-API for VPP output. Forward mapping dynamic VA-API pools is not
      // supported by all oneVPL/FFmpeg combinations. Direct reverse mapping
      // keeps the NV12 surfaces in GPU memory and pins the selected VA device.
      filters.emplace_back(
          "hwmap",
          "derive_device=qsv:reverse=1:mode=read+direct:extra_hw_frames=4");
    }
  } else {
    if (first.format == AV_PIX_FMT_DRM_PRIME)
      throw std::runtime_error("DRM input needs VA-API or Intel QSV");
    filters.emplace_back("scale",
                         std::to_string(width) + ":" + std::to_string(height) +
                             ":out_color_matrix=bt709:out_range=limited");
    filters.emplace_back("format", backend == "nvenc" ? "pix_fmts=nv12"
                                                      : "pix_fmts=yuv420p");
  }
  // Reverse VA-API/QSV mapping negotiates from the consumer's format.
  // Constrain it explicitly; an unconstrained sink can choose VA-API on the
  // derived QSV device, which cannot allocate VA-API frames itself.
  check(avfilter_graph_create_filter(
            &sink, avfilter_get_by_name("buffersink"), "encoded",
            backend == "qsv" ? "pixel_formats=qsv" : nullptr, nullptr, graph),
        "Create video sink");
  auto previousFilter = input;
  for (const auto &description : filters) {
    auto filter = avfilter_get_by_name(description.first.c_str());
    if (!filter)
      throw std::runtime_error("Required FFmpeg filter is unavailable: " +
                               description.first);
    auto next =
        avfilter_graph_alloc_filter(graph, filter, description.first.c_str());
    if (!next)
      throw std::bad_alloc();
    if (device)
      next->hw_device_ctx = av_buffer_ref(device);
    check(avfilter_init_str(next, description.second.empty()
                                      ? nullptr
                                      : description.second.c_str()),
          "Initialize " + description.first);
    check(avfilter_link(previousFilter, 0, next, 0),
          "Connect " + description.first);
    previousFilter = next;
  }
  check(avfilter_link(previousFilter, 0, sink, 0), "Connect encoder input");
  check(avfilter_graph_config(graph, nullptr), "Configure capture conversion");
  codec = avcodec_alloc_context3(implementation);
  if (!codec)
    throw std::bad_alloc();
  codec->width = width;
  codec->height = height;
  codec->time_base = {1, 1000000};
  codec->framerate = {o.fps, 1};
  codec->pix_fmt = static_cast<AVPixelFormat>(av_buffersink_get_format(sink));
  inputFormat = av_get_pix_fmt_name(codec->pix_fmt);
  if (auto hw = av_buffersink_get_hw_frames_ctx(sink))
    codec->hw_frames_ctx = av_buffer_ref(hw);
  codec->bit_rate = o.bitrate;
  codec->gop_size = o.fps * 2;
  codec->max_b_frames = 0;
  codec->color_range = AVCOL_RANGE_MPEG;
  codec->colorspace = AVCOL_SPC_BT709;
  codec->color_primaries = AVCOL_PRI_BT709;
  codec->color_trc = AVCOL_TRC_BT709;
  codec->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
  AVDictionary *settings = nullptr;
  if (backend == "software") {
    if (o.videoCodec == "h264" || o.videoCodec == "h265") {
      av_dict_set(&settings, "preset", "veryfast", 0);
      if (o.videoCodec == "h264")
        av_dict_set(&settings, "tune", "zerolatency", 0);
      else
        av_dict_set(
            &settings, "x265-params",
            "log-level=error:pools=2:frame-threads=1:bframes=0:rc-lookahead=0",
            0);
    } else if (o.videoCodec == "vp9") {
      av_dict_set(&settings, "deadline", "realtime", 0);
      av_dict_set(&settings, "cpu-used", "6", 0);
      av_dict_set(&settings, "lag-in-frames", "0", 0);
    } else {
      av_dict_set(&settings, "preset", "10", 0);
      av_dict_set(&settings, "svtav1-params", "lp=2", 0);
    }
  }
  if (backend == "nvenc") {
    av_dict_set(&settings, "preset", "p4", 0);
    av_dict_set(&settings, "tune", "ll", 0);
  }
  if (backend == "vaapi")
    av_dict_set(&settings, "rc_mode", "VBR", 0);
  if (backend == "qsv") {
    // Bound latency and surface retention: the eye renderer has four targets.
    av_dict_set(&settings, "async_depth", "1", 0);
    if (o.videoCodec == "h264")
      av_dict_set(&settings, "look_ahead", "0", 0);
    av_dict_set(&settings, "preset", "fast", 0);
    codec->rc_max_rate = int64_t(o.bitrate) * 3 / 2;
    codec->rc_buffer_size = o.bitrate * 2;
  }
  status = avcodec_open2(codec, implementation, &settings);
  std::string ignored;
  for (auto entry = av_dict_get(settings, "", nullptr, AV_DICT_IGNORE_SUFFIX);
       entry; entry = av_dict_get(settings, "", entry, AV_DICT_IGNORE_SUFFIX))
    ignored += std::string(entry->key) + " ";
  av_dict_free(&settings);
  check(status, "Open hardware encoder " + name);
  if (!ignored.empty())
    throw std::runtime_error("Encoder does not support requested settings: " +
                             ignored);
  check(avformat_alloc_output_context2(&mux, nullptr, "matroska",
                                       o.output.c_str()),
        "Create Matroska output");
  auto stream = avformat_new_stream(mux, nullptr);
  if (!stream)
    throw std::bad_alloc();
  stream->time_base = codec->time_base;
  stream->avg_frame_rate = codec->framerate;
  check(avcodec_parameters_from_context(stream->codecpar, codec),
        "Write codec parameters");
  if (o.videoCodec == "vp9") {
    auto filter = av_bsf_get_by_name("vp9_metadata");
    if (!filter)
      throw std::runtime_error(
          "VP9 capture requires FFmpeg's vp9_metadata filter");
    check(av_bsf_alloc(filter, &videoBsf), "Create VP9 metadata filter");
    check(avcodec_parameters_copy(videoBsf->par_in, stream->codecpar),
          "Set VP9 bitstream parameters");
    videoBsf->time_base_in = codec->time_base;
    check(av_opt_set(videoBsf->priv_data, "color_space", "bt709", 0),
          "Set VP9 bitstream colour space");
    check(av_opt_set(videoBsf->priv_data, "color_range", "tv", 0),
          "Set VP9 bitstream range");
    check(av_bsf_init(videoBsf), "Initialize VP9 metadata filter");
    check(avcodec_parameters_copy(stream->codecpar, videoBsf->par_out),
          "Set filtered VP9 stream parameters");
  }
  if (o.audio != "off") {
    auto opus = avcodec_find_encoder_by_name("libopus");
    if (!opus)
      throw std::runtime_error(
          "Audio capture requires FFmpeg's libopus encoder");
    audioCodec = avcodec_alloc_context3(opus);
    if (!audioCodec)
      throw std::bad_alloc();
    audioCodec->sample_fmt = AV_SAMPLE_FMT_FLT;
    audioCodec->sample_rate = 48000;
    av_channel_layout_default(&audioCodec->ch_layout, 2);
    audioCodec->time_base = {1, 48000};
    audioCodec->bit_rate = 128000;
    audioCodec->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
    check(avcodec_open2(audioCodec, opus, nullptr), "Open Opus audio encoder");
    auto audioStream = avformat_new_stream(mux, nullptr);
    if (!audioStream)
      throw std::bad_alloc();
    audioStream->time_base = audioCodec->time_base;
    check(avcodec_parameters_from_context(audioStream->codecpar, audioCodec),
          "Write audio codec parameters");
    audioFifo = av_audio_fifo_alloc(AV_SAMPLE_FMT_FLT, 2, 1920);
    if (!audioFifo)
      throw std::bad_alloc();
  }
  // O_EXCL prevents accidental clobbering, including symlink destinations.
  fd = ::open(o.output.c_str(),
              O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
  if (fd < 0)
    throw std::runtime_error("Create output: " + std::string(strerror(errno)));
  auto buffer = static_cast<unsigned char *>(av_malloc(65536));
  if (!buffer)
    throw std::bad_alloc();
  io =
      avio_alloc_context(buffer, 65536, 1, &fd, nullptr, writeBytes, seekBytes);
  if (!io) {
    av_free(buffer);
    throw std::bad_alloc();
  }
  mux->pb = io;
  mux->flags |= AVFMT_FLAG_CUSTOM_IO;
  check(avformat_write_header(mux, nullptr), "Write recording header");
  header = true;
}
void Encoder::packets(AVCodecContext *context, int streamIndex) {
  AVPacket *packet = av_packet_alloc();
  if (!packet)
    throw std::bad_alloc();
  try {
    for (;;) {
      int status = avcodec_receive_packet(context, packet);
      if (status == AVERROR(EAGAIN) || status == AVERROR_EOF)
        break;
      check(status, "Read encoded packet");
      if (context == codec && videoBsf)
        filteredPackets(packet);
      else
        muxPacket(packet, context->time_base, streamIndex);
      av_packet_unref(packet);
    }
  } catch (...) {
    av_packet_free(&packet);
    throw;
  }
  av_packet_free(&packet);
}
void Encoder::muxPacket(AVPacket *packet, AVRational timeBase,
                        int streamIndex) {
  bytes += packet->size;
  av_packet_rescale_ts(packet, timeBase, mux->streams[streamIndex]->time_base);
  packet->stream_index = streamIndex;
  check(av_interleaved_write_frame(mux, packet), "Write recording");
}
void Encoder::filteredPackets(AVPacket *inputPacket) {
  check(av_bsf_send_packet(videoBsf, inputPacket),
        "Set VP9 bitstream metadata");
  AVPacket *packet = av_packet_alloc();
  if (!packet)
    throw std::bad_alloc();
  try {
    for (;;) {
      int status = av_bsf_receive_packet(videoBsf, packet);
      if (status == AVERROR(EAGAIN) || status == AVERROR_EOF)
        break;
      check(status, "Read filtered VP9 packet");
      muxPacket(packet, videoBsf->time_base_out, 0);
      av_packet_unref(packet);
    }
  } catch (...) {
    av_packet_free(&packet);
    throw;
  }
  av_packet_free(&packet);
}
void Encoder::write(AVFrame &source, int64_t ptsUs) {
  if (finished || source.width != inWidth || source.height != inHeight ||
      source.format != inFormat)
    throw std::runtime_error("Source format changed; recording stopped safely");
  source.pts = std::max(previous + 1, ptsUs);
  previous = source.pts;
  source.color_range = AVCOL_RANGE_JPEG;
  source.colorspace = AVCOL_SPC_RGB;
  source.color_primaries = AVCOL_PRI_BT709;
  source.color_trc = AVCOL_TRC_IEC61966_2_1;
  check(
      av_buffersrc_add_frame_flags(input, &source, AV_BUFFERSRC_FLAG_KEEP_REF),
      "Submit capture frame");
  auto converted = frame();
  for (;;) {
    int status = av_buffersink_get_frame(sink, converted.get());
    if (status == AVERROR(EAGAIN) || status == AVERROR_EOF)
      break;
    check(status, "GPU colour conversion");
    check(avcodec_send_frame(codec, converted.get()), "Encode capture frame");
    ++frames;
    packets(codec, 0);
    av_frame_unref(converted.get());
  }
}
void Encoder::writeAudio(AudioBlock block, uint64_t epoch) {
  if (!audioCodec || finished)
    throw std::runtime_error("Audio stream is not open");
  if (block.samples.size() % 2)
    throw std::runtime_error("Invalid interleaved audio block");
  int64_t target = av_rescale_q(int64_t(block.timestamp) - int64_t(epoch),
                                {1, 1000000000}, {1, 48000});
  int64_t tail = audioSamples + av_audio_fifo_size(audioFifo);
  size_t skip = target < tail
                    ? std::min<int64_t>(block.samples.size() / 2, tail - target)
                    : 0;
  if (target > tail) {
    if (target - tail > 48000 * 2)
      throw std::runtime_error("Audio clock discontinuity exceeds two seconds");
    std::vector<float> silence((target - tail) * 2, 0);
    void *data[] = {silence.data()};
    check(av_audio_fifo_write(audioFifo, data, target - tail),
          "Fill audio timestamp gap");
  }
  int count = block.samples.size() / 2 - skip;
  if (count) {
    void *data[] = {block.samples.data() + skip * 2};
    check(av_audio_fifo_write(audioFifo, data, count), "Queue audio samples");
  }
  audioFrames(false);
}
void Encoder::audioFrames(bool final) {
  const int count = audioCodec->frame_size;
  while (av_audio_fifo_size(audioFifo) >= count ||
         (final && av_audio_fifo_size(audioFifo))) {
    auto pcm = frame();
    pcm->format = audioCodec->sample_fmt;
    pcm->sample_rate = 48000;
    pcm->nb_samples = count;
    check(av_channel_layout_copy(&pcm->ch_layout, &audioCodec->ch_layout),
          "Set audio channel layout");
    check(av_frame_get_buffer(pcm.get(), 0), "Allocate encoded audio frame");
    int available = std::min(count, av_audio_fifo_size(audioFifo));
    check(av_audio_fifo_read(audioFifo, reinterpret_cast<void **>(pcm->data),
                             available),
          "Read queued audio");
    if (available < count)
      check(av_samples_set_silence(pcm->data, available, count - available, 2,
                                   audioCodec->sample_fmt),
            "Pad final audio frame");
    pcm->pts = audioSamples;
    audioSamples += count;
    check(avcodec_send_frame(audioCodec, pcm.get()), "Encode captured audio");
    packets(audioCodec, 1);
  }
}
void Encoder::finish() {
  if (finished || !header)
    return;
  finished = true;
  check(avcodec_send_frame(codec, nullptr), "Flush encoder");
  packets(codec, 0);
  if (videoBsf)
    filteredPackets(nullptr);
  if (audioCodec) {
    audioFrames(true);
    check(avcodec_send_frame(audioCodec, nullptr), "Flush audio encoder");
    packets(audioCodec, 1);
  }
  check(av_write_trailer(mux), "Finalize Matroska recording");
  avio_flush(io);
  check(io->error, "Flush recording");
  if (fsync(fd) < 0)
    throw std::runtime_error("Sync recording: " + std::string(strerror(errno)));
}
Frame testFrame(int w, int h) {
  auto f = frame();
  f->format = AV_PIX_FMT_RGBA;
  f->width = w;
  f->height = h;
  check(av_frame_get_buffer(f.get(), 32), "Allocate test frame");
  for (int y = 0; y < h; ++y)
    for (int x = 0; x < w; ++x) {
      auto p = f->data[0] + y * f->linesize[0] + x * 4;
      p[0] = x < w / 2 ? 240 : 16;
      p[1] = 32;
      p[2] = x < w / 2 ? 16 : 240;
      p[3] = 255;
    }
  return f;
}
} // namespace capture
