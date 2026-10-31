// SPDX-License-Identifier: MIT
#include "capture.h"
#include "third_party/obsmirror/frame_ipc.h"
#include <cassert>
#include <filesystem>
#include <fstream>
#include <sys/stat.h>
#include <unistd.h>
int main() {
  char temporary[] = "/tmp/dome-capture-test-XXXXXX";
  assert(mkdtemp(temporary));
  setenv("OPENXR_OBS_MIRROR_DIR", temporary, 1);
  {
    linux_mirror::Frame f;
    f.header.width = 10;
    f.header.height = 6;
    f.header.leftWidth = 4;
    f.header.rightWidth = 6;
    f.header.stride = 40;
    f.pixels.resize(240);
    for (int y = 0; y < 6; ++y)
      for (int x = 0; x < 10; ++x) {
        auto p = f.pixels.data() + y * 40 + x * 4;
        p[0] = x < 4 ? 240 : 16;
        p[1] = y * 20;
        p[2] = x < 4 ? 16 : 240;
        p[3] = 255;
      }
    linux_mirror::FrameWriter writer;
    assert(writer.publish(f));
    uint64_t sequence = 0;
    auto left = capture::readEye(writer.path(), sequence, 0);
    assert(left && left->width == 4 && left->height == 6 &&
           left->data[0][0] == 240);
    assert(left->data[0][5 * left->linesize[0] + 1] == 100);
    assert(!capture::readEye(writer.path(), sequence, 0));
    sequence = 0;
    auto right = capture::readEye(writer.path(), sequence, 1);
    assert(right && right->width == 6 && right->data[0][2] == 240 &&
           right->data[0][0] == 16);
    auto symlink = std::string(temporary) + "/link";
    assert(::symlink(writer.path().c_str(), symlink.c_str()) == 0);
    sequence = 0;
    assert(!capture::readEye(symlink, sequence, 0));
    assert(chmod(writer.path().c_str(), 0644) == 0);
    assert(!capture::readEye(writer.path(), sequence, 0));
    assert(chmod(writer.path().c_str(), 0600) == 0);
  }
  capture::Options o;
  o.source = "test";
  o.encoder = "software";
  o.output = std::string(temporary) + "/video-ž.mkv";
  o.seconds = .3;
  capture::Job job;
  job.start(o);
  job.join();
  auto state = job.snapshot();
  assert(!state.contains("error") && state["frames"].get<int>() >= 5 &&
         state["width"] == 320);
  auto original = std::filesystem::file_size(o.output);
  capture::Job duplicate;
  duplicate.start(o);
  duplicate.join();
  assert(duplicate.snapshot().contains("error"));
  assert(std::filesystem::file_size(o.output) == original);
  bool rejected = false;
  try {
    o.fps = 0;
    o.validate();
  } catch (...) {
    rejected = true;
  }
  assert(rejected);
  // QSV accepts the same direct GPU XR source; software and NVENC remain
  // restricted to CPU input. Misspelled encoders must never select software.
  o.fps = 30;
  o.source = "xr";
  o.encoder = "qsv";
  o.validate();
  rejected = false;
  try {
    o.encoder = "qs v";
    o.validate();
  } catch (...) {
    rejected = true;
  }
  assert(rejected);
  std::filesystem::remove_all(temporary);
  puts("PASS: selected eye, unequal widths, stride, sequence, symlinks, "
       "permissions, Unicode output, encoder, no-clobber, limits");
}
