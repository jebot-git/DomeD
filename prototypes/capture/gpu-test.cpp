// SPDX-License-Identifier: MIT
// Optional real-GPU integration fixture, requiring a usable VA-API render node.
// It renders flat/cylinder SBS surfaces through the actual DMA-BUF eye path.
#include "capture.h"
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <GLES2/gl2ext.h>
#include <arpa/inet.h>
#include <cassert>
#include <drm_fourcc.h>
#include <fcntl.h>
#include <filesystem>
#include <gbm.h>
#include <iostream>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>
namespace {
using capture::Json;
void sendAll(int fd, const void *p, size_t size) {
  auto bytes = static_cast<const uint8_t *>(p);
  while (size) {
    auto n = send(fd, bytes, size, MSG_NOSIGNAL);
    if (n < 0 && errno == EINTR)
      continue;
    if (n <= 0)
      throw std::runtime_error("Fixture write failed");
    bytes += n;
    size -= n;
  }
}
struct Fixture {
  int device = -1, dmabuf = -1, http = -1, gpu = -1;
  gbm_device *gbm = nullptr;
  gbm_bo *bo = nullptr;
  std::string directory, path, server;
  std::thread httpThread, gpuThread;
  std::vector<int> leases;
  std::atomic<bool> curved{false}, revoked{false};
  Json metadata;
  explicit Fixture(const std::string &node) {
    char temporary[] = "/tmp/dome-capture-gpu-XXXXXX";
    assert(mkdtemp(temporary));
    directory = temporary;
    path = directory + "/gpu.sock";
    device = open(node.c_str(), O_RDWR | O_CLOEXEC);
    assert(device >= 0);
    gbm = gbm_create_device(device);
    assert(gbm);
    bo = gbm_bo_create(gbm, 640, 320, DRM_FORMAT_ARGB8888,
                       GBM_BO_USE_RENDERING | GBM_BO_USE_LINEAR);
    assert(bo);
    dmabuf = gbm_bo_get_fd(bo);
    assert(dmabuf >= 0);
    // Produce an immutable test image entirely on the GPU too. Row zero is
    // green, and each eye has a distinct colour below it.
    auto platform = reinterpret_cast<PFNEGLGETPLATFORMDISPLAYEXTPROC>(
        eglGetProcAddress("eglGetPlatformDisplayEXT"));
    assert(platform);
    auto display = platform(EGL_PLATFORM_GBM_KHR, gbm, nullptr);
    assert(eglInitialize(display, nullptr, nullptr));
    eglBindAPI(EGL_OPENGL_ES_API);
    EGLint configAttributes[] = {EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
                                 EGL_SURFACE_TYPE, 0, EGL_NONE};
    EGLConfig config;
    EGLint count;
    assert(eglChooseConfig(display, configAttributes, &config, 1, &count) &&
           count);
    EGLint version[] = {EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE};
    auto context = eglCreateContext(display, config, EGL_NO_CONTEXT, version);
    assert(context != EGL_NO_CONTEXT);
    assert(eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, context));
    auto createImage = reinterpret_cast<PFNEGLCREATEIMAGEKHRPROC>(
        eglGetProcAddress("eglCreateImageKHR"));
    auto destroyImage = reinterpret_cast<PFNEGLDESTROYIMAGEKHRPROC>(
        eglGetProcAddress("eglDestroyImageKHR"));
    auto imageTexture = reinterpret_cast<PFNGLEGLIMAGETARGETTEXTURE2DOESPROC>(
        eglGetProcAddress("glEGLImageTargetTexture2DOES"));
    auto modifier = gbm_bo_get_modifier(bo);
    EGLint attributes[] = {EGL_WIDTH,
                           640,
                           EGL_HEIGHT,
                           320,
                           EGL_LINUX_DRM_FOURCC_EXT,
                           EGLint(DRM_FORMAT_ARGB8888),
                           EGL_DMA_BUF_PLANE0_FD_EXT,
                           dmabuf,
                           EGL_DMA_BUF_PLANE0_OFFSET_EXT,
                           0,
                           EGL_DMA_BUF_PLANE0_PITCH_EXT,
                           EGLint(gbm_bo_get_stride(bo)),
                           EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT,
                           EGLint(modifier),
                           EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT,
                           EGLint(modifier >> 32),
                           EGL_NONE};
    auto image = createImage(display, EGL_NO_CONTEXT, EGL_LINUX_DMA_BUF_EXT,
                             nullptr, attributes);
    assert(image != EGL_NO_IMAGE_KHR);
    GLuint texture, framebuffer;
    glGenTextures(1, &texture);
    glBindTexture(GL_TEXTURE_2D, texture);
    imageTexture(GL_TEXTURE_2D, image);
    glGenFramebuffers(1, &framebuffer);
    glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D,
                           texture, 0);
    assert(glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE);
    glViewport(0, 0, 640, 320);
    glEnable(GL_SCISSOR_TEST);
    glScissor(0, 0, 320, 320);
    glClearColor(.94, .125, .06, 1);
    glClear(GL_COLOR_BUFFER_BIT);
    glScissor(320, 0, 320, 320);
    glClearColor(.06, .125, .94, 1);
    glClear(GL_COLOR_BUFFER_BIT);
    glScissor(0, 0, 640, 80);
    glClearColor(.06, .94, .06, 1);
    glClear(GL_COLOR_BUFFER_BIT);
    glFinish();
    assert(glGetError() == GL_NO_ERROR);
    glDeleteFramebuffers(1, &framebuffer);
    glDeleteTextures(1, &texture);
    destroyImage(display, image);
    eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    eglDestroyContext(display, context);
    eglTerminate(display);
    struct stat st{};
    assert(fstat(device, &st) == 0);
    metadata = {{"version", 1},
                {"serial", 1},
                {"device", uint64_t(st.st_rdev)},
                {"width", 640},
                {"height", 320},
                {"logical_width", 640},
                {"logical_height", 320},
                {"fourcc", DRM_FORMAT_ARGB8888},
                {"modifier", gbm_bo_get_modifier(bo)},
                {"stride", gbm_bo_get_stride(bo)},
                {"offset", 0},
                {"y_inverted", false},
                {"viewport", nullptr}};
    http = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    assert(http >= 0);
    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    assert(bind(http, reinterpret_cast<sockaddr *>(&address),
                sizeof(address)) == 0);
    socklen_t size = sizeof(address);
    assert(getsockname(http, reinterpret_cast<sockaddr *>(&address), &size) ==
           0);
    assert(listen(http, 8) == 0);
    server = "http://127.0.0.1:" + std::to_string(ntohs(address.sin_port));
    gpu = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    assert(gpu >= 0);
    sockaddr_un unixAddress{};
    unixAddress.sun_family = AF_UNIX;
    strcpy(unixAddress.sun_path, path.c_str());
    assert(bind(gpu, reinterpret_cast<sockaddr *>(&unixAddress),
                sizeof(unixAddress)) == 0);
    assert(listen(gpu, 8) == 0);
    httpThread = std::thread([this] {
      for (;;) {
        int peer = accept4(http, nullptr, nullptr, SOCK_CLOEXEC);
        if (peer < 0)
          break;
        char request[4096];
        recv(peer, request, sizeof(request), 0);
        Json pose = {{"position_m", {0, 0, -2}},
                     {"orientation_xyzw", {0, 0, 0, 1}}};
        Json eye = pose;
        eye["position_m"] = {0, 0, 0};
        Json left = eye, right = eye;
        left["position_m"] = {-.032, 0, 0};
        right["position_m"] = {.032, 0, 0};
        Json dome = nullptr;
        if (curved) {
          eye["position_m"] = {0, 0, 0};
          dome = {{"frame", eye},
                  {"radius", 2},
                  {"horizontal", 1.7},
                  {"top", 1.},
                  {"bottom", -1.}};
        }
        Json quad = {
            {"id", 1}, {"pose", pose}, {"size", {4, 4}}, {"dome", dome}};
        Json j = {{"version", 1},
                  {"available", !revoked},
                  {"reason", "Fixture revoked"},
                  {"gpu_socket", path},
                  {"eyes", {left, right}},
                  {"viewport", {-.7, .7, -.7, .7}},
                  {"surfaces",
                   Json::array(
                       {{{"quad", quad}, {"stereo", true}, {"opacity", 1.}}})}};
        auto body = j.dump();
        auto header = "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: "
                      "application/json\r\nContent-Length: " +
                      std::to_string(body.size()) + "\r\n\r\n";
        try {
          sendAll(peer, header.data(), header.size());
          sendAll(peer, body.data(), body.size());
        } catch (...) {
        }
        close(peer);
      }
    });
    gpuThread = std::thread([this] {
      for (;;) {
        int peer = accept4(gpu, nullptr, nullptr, SOCK_CLOEXEC);
        if (peer < 0)
          break;
        char id[8];
        recv(peer, id, sizeof(id), MSG_WAITALL);
        uint8_t byte = 1;
        iovec iov{&byte, 1};
        char control[CMSG_SPACE(sizeof(int))]{};
        msghdr message{};
        message.msg_iov = &iov;
        message.msg_iovlen = 1;
        message.msg_control = control;
        message.msg_controllen = sizeof(control);
        auto c = CMSG_FIRSTHDR(&message);
        c->cmsg_level = SOL_SOCKET;
        c->cmsg_type = SCM_RIGHTS;
        c->cmsg_len = CMSG_LEN(sizeof(int));
        memcpy(CMSG_DATA(c), &dmabuf, sizeof(int));
        if (sendmsg(peer, &message, MSG_NOSIGNAL) < 0) {
          close(peer);
          continue;
        }
        auto text = metadata.dump();
        uint32_t n = text.size();
        uint8_t length[4];
        for (int i = 0; i < 4; ++i)
          length[i] = n >> (i * 8);
        try {
          sendAll(peer, length, 4);
          sendAll(peer, text.data(), text.size());
          leases.push_back(peer);
        } catch (...) {
          close(peer);
        }
      }
    });
  }
  ~Fixture() {
    shutdown(http, SHUT_RDWR);
    shutdown(gpu, SHUT_RDWR);
    httpThread.join();
    gpuThread.join();
    for (int fd : leases)
      close(fd);
    close(http);
    close(gpu);
    close(dmabuf);
    gbm_bo_destroy(bo);
    gbm_device_destroy(gbm);
    close(device);
    std::filesystem::remove_all(directory);
  }
};
} // namespace
int main(int argc, char **argv) try {
  if (argc < 3 || argc > 5)
    throw std::runtime_error(
        "capture-gpu-test /dev/dri/renderD129 OUTPUT_DIRECTORY [vaapi|qsv] "
        "[h264|h265|vp9|av1]");
  std::filesystem::create_directories(argv[2]);
  Fixture fixture(argv[1]);
  for (bool curved : {false, true})
    for (int eye : {0, 1}) {
      fixture.curved = curved;
      capture::Options options;
      options.source = "xr";
      options.encoder = argc >= 4 ? argv[3] : "vaapi";
      options.videoCodec = argc == 5 ? argv[4] : "h264";
      if (options.encoder != "vaapi" && options.encoder != "qsv")
        throw std::runtime_error("GPU test encoder must be vaapi or qsv");
      options.device = argv[1];
      options.server = fixture.server;
      options.eye = eye;
      options.width = 320;
      auto name =
          std::string(curved ? "curved-" : "flat-") + (eye ? "right" : "left");
      options.output =
          (std::filesystem::path(argv[2]) / (name + ".mkv")).string();
      options.validate();
      auto source = capture::makeEyeSource(options);
      capture::Encoder encoder;
      for (int i = 0; i < 40; ++i) {
        auto f = source->next();
        assert(f && f->format == AV_PIX_FMT_DRM_PRIME);
        if (!i)
          encoder.open(options, *f);
        encoder.write(*f, i * 33333);
      }
      encoder.finish();
      assert(encoder.frames == 40);
      assert(encoder.inputFormat ==
             (options.encoder == "qsv" ? "qsv" : "vaapi"));
      fixture.revoked = true;
      bool stopped = false;
      try {
        source->next();
      } catch (const std::runtime_error &) {
        stopped = true;
      }
      assert(stopped);
      fixture.revoked = false;
      std::cout << "PASS " << name
                << ": 40 GPU frames, bounded pool reuse and revocation\n";
    }
} catch (const std::exception &e) {
  std::cerr << e.what() << '\n';
  return 1;
}
