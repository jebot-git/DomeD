// SPDX-License-Identifier: MIT
// Flatten DomeD's own leased DMA-BUF surfaces into a single selected eye.
// The output is a DRM PRIME frame mapped directly into VA-API, without
// readback.
#include "capture.h"
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <GLES2/gl2ext.h>
#include <curl/curl.h>
#include <drm_fourcc.h>
#include <gbm.h>
extern "C" {
#include <libavutil/hwcontext_drm.h>
}
#include <algorithm>
#include <array>
#include <cmath>
#include <fcntl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>
namespace capture {
namespace {
struct Fd {
  int value = -1;
  explicit Fd(int n = -1) : value(n) {}
  ~Fd() {
    if (value >= 0)
      close(value);
  }
  Fd(const Fd &) = delete;
  Fd &operator=(const Fd &) = delete;
};
void readAll(int fd, void *data, size_t count) {
  auto p = static_cast<uint8_t *>(data);
  while (count) {
    auto n = recv(fd, p, count, 0);
    if (n < 0 && errno == EINTR)
      continue;
    if (n <= 0)
      throw std::runtime_error("Truncated GPU frame metadata");
    p += n;
    count -= n;
  }
}
struct Lease {
  Fd socket, buffer;
  Json info;
};
std::unique_ptr<Lease> receive(const std::string &path, uint64_t id) {
  if (path.empty() || path.size() >= sizeof(sockaddr_un::sun_path))
    throw std::runtime_error("Invalid GPU socket path");
  auto l = std::make_unique<Lease>();
  l->socket.value = ::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
  if (l->socket.value < 0)
    throw std::runtime_error("Create GPU socket");
  sockaddr_un address{};
  address.sun_family = AF_UNIX;
  memcpy(address.sun_path, path.c_str(), path.size() + 1);
  timeval timeout{0, 250000};
  setsockopt(l->socket.value, SOL_SOCKET, SO_RCVTIMEO, &timeout,
             sizeof(timeout));
  setsockopt(l->socket.value, SOL_SOCKET, SO_SNDTIMEO, &timeout,
             sizeof(timeout));
  if (connect(l->socket.value, reinterpret_cast<sockaddr *>(&address),
              sizeof(address)))
    throw std::runtime_error("Connect DomeD GPU socket");
  ucred peer{};
  socklen_t length = sizeof(peer);
  if (getsockopt(l->socket.value, SOL_SOCKET, SO_PEERCRED, &peer, &length) ||
      peer.uid != getuid())
    throw std::runtime_error("GPU transport peer is not the current user");
  uint8_t request[8];
  for (unsigned i = 0; i < 8; ++i)
    request[i] = id >> (i * 8);
  if (send(l->socket.value, request, 8, MSG_NOSIGNAL) != 8)
    throw std::runtime_error("Request GPU surface");
  uint8_t byte = 0;
  char control[CMSG_SPACE(sizeof(int) * 4)]{};
  iovec iov{&byte, 1};
  msghdr message{};
  message.msg_iov = &iov;
  message.msg_iovlen = 1;
  message.msg_control = control;
  message.msg_controllen = sizeof(control);
  auto size = recvmsg(l->socket.value, &message, MSG_CMSG_CLOEXEC);
  unsigned descriptors = 0;
  for (auto c = CMSG_FIRSTHDR(&message); c; c = CMSG_NXTHDR(&message, c)) {
    if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS &&
        c->cmsg_len >= CMSG_LEN(0)) {
      size_t count = (c->cmsg_len - CMSG_LEN(0)) / sizeof(int);
      auto fds = reinterpret_cast<int *>(CMSG_DATA(c));
      for (size_t i = 0; i < count; ++i) {
        ++descriptors;
        if (l->buffer.value < 0)
          l->buffer.value = fds[i];
        else
          close(fds[i]);
      }
    }
  }
  if (size == 1 && byte == 0 && !descriptors)
    return {};
  if (size != 1 || byte != 1 || descriptors != 1 ||
      (message.msg_flags & MSG_CTRUNC))
    throw std::runtime_error("Invalid GPU descriptor message");
  uint8_t raw[4];
  readAll(l->socket.value, raw, 4);
  uint32_t count = uint32_t(raw[0]) | (uint32_t(raw[1]) << 8) |
                   (uint32_t(raw[2]) << 16) | (uint32_t(raw[3]) << 24);
  if (!count || count > 4096)
    throw std::runtime_error("Invalid GPU metadata length");
  std::string text(count, '\0');
  readAll(l->socket.value, text.data(), count);
  l->info = Json::parse(text);
  auto &j = l->info;
  uint64_t w = j.at("width"), h = j.at("height"), stride = j.at("stride"),
           offset = j.at("offset");
  if (j.at("version") != 1 || !w || !h || w > 4096 || h > 4096 ||
      stride < w * 4 || stride > 65536 || offset > 256 * 1024 * 1024)
    throw std::runtime_error("Invalid GPU frame geometry");
  auto fourcc = j.at("fourcc").get<uint32_t>();
  if (fourcc != DRM_FORMAT_ARGB8888 && fourcc != DRM_FORMAT_XRGB8888 &&
      fourcc != DRM_FORMAT_ABGR8888 && fourcc != DRM_FORMAT_XBGR8888)
    throw std::runtime_error("Unsupported GPU frame format");
  return l;
}
struct Vec {
  float x = 0, y = 0, z = 0;
  Vec operator+(Vec b) const { return {x + b.x, y + b.y, z + b.z}; }
  Vec operator-(Vec b) const { return {x - b.x, y - b.y, z - b.z}; }
  Vec operator*(float n) const { return {x * n, y * n, z * n}; }
};
Vec cross(Vec a, Vec b) {
  return {a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x};
}
struct Pose {
  Vec position;
  std::array<float, 4> q;
  explicit Pose(const Json &p) {
    auto a = p.at("position_m").get<std::array<float, 3>>();
    position = {a[0], a[1], a[2]};
    q = p.at("orientation_xyzw");
    float norm = 0;
    for (auto f : q)
      norm += f * f;
    if (!std::isfinite(norm) || norm < 0.99f || norm > 1.01f)
      throw std::runtime_error("Invalid XR orientation");
  }
  Vec rotate(Vec p, bool inverse = false) const {
    Vec v{q[0], q[1], q[2]};
    if (inverse)
      v = v * -1;
    auto t = cross(v, p) * 2;
    return p + t * q[3] + cross(v, t);
  }
  Vec world(Vec p) const { return position + rotate(p); }
  Vec view(Vec p) const { return rotate(p - position, true); }
};
struct Device {
  Fd fd;
  gbm_device *gbm = nullptr;
  explicit Device(const std::string &path)
      : fd(::open(path.c_str(), O_RDWR | O_CLOEXEC)) {
    if (fd.value < 0 || !(gbm = gbm_create_device(fd.value)))
      throw std::runtime_error("Open GBM capture GPU " + path);
  }
  ~Device() {
    if (gbm)
      gbm_device_destroy(gbm);
  }
};
struct Target {
  std::shared_ptr<Device> device;
  gbm_bo *bo = nullptr;
  explicit Target(std::shared_ptr<Device> d, int w, int h)
      : device(std::move(d)) {
    bo = gbm_bo_create(device->gbm, w, h, DRM_FORMAT_ARGB8888,
                       GBM_BO_USE_RENDERING | GBM_BO_USE_LINEAR);
    if (!bo || gbm_bo_get_plane_count(bo) != 1) {
      if (bo)
        gbm_bo_destroy(bo);
      bo = nullptr;
      throw std::runtime_error("No renderable single-plane GBM target");
    }
  }
  ~Target() {
    if (bo)
      gbm_bo_destroy(bo);
  }
};
struct Payload {
  AVDRMFrameDescriptor descriptor{};
  std::shared_ptr<Target> target;
  ~Payload() {
    if (descriptor.nb_objects)
      close(descriptor.objects[0].fd);
  }
};
class Eye final : public Source {
  Options options;
  std::shared_ptr<Device> device;
  EGLDisplay display = EGL_NO_DISPLAY;
  EGLContext context = EGL_NO_CONTEXT;
  PFNEGLCREATEIMAGEKHRPROC createImage = nullptr;
  PFNEGLDESTROYIMAGEKHRPROC destroyImage = nullptr;
  PFNGLEGLIMAGETARGETTEXTURE2DOESPROC imageTexture = nullptr;
  GLuint program = 0, vbo = 0, fbo = 0, depth = 0;
  int width = 0, height = 0;
  AVBufferRef *drmDevice = nullptr, *drmFrames = nullptr;
  std::vector<std::shared_ptr<Target>> targets;
  struct Texture {
    GLuint name = 0;
    EGLImageKHR image = EGL_NO_IMAGE_KHR;
    std::unique_ptr<Lease> lease;
  };
  std::map<uint64_t, Texture> textures;
  Json snapshot() {
    auto curl = curl_easy_init();
    if (!curl)
      throw std::bad_alloc();
    std::string body;
    auto url = options.server + "/api/capture/xr";
    curl_easy_setopt(curl, CURLOPT_URL, url.c_str());
    curl_easy_setopt(curl, CURLOPT_NOPROXY, "*");
    curl_easy_setopt(curl, CURLOPT_TIMEOUT_MS, 500L);
    curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT_MS, 250L);
    curl_easy_setopt(curl, CURLOPT_NOSIGNAL, 1L);
    curl_easy_setopt(
        curl, CURLOPT_WRITEFUNCTION,
        +[](char *data, size_t size, size_t count, void *user) -> size_t {
          auto &s = *static_cast<std::string *>(user);
          size_t n = size * count;
          if (n > 2 * 1024 * 1024 - s.size())
            return 0;
          s.append(data, n);
          return n;
        });
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &body);
    auto status = curl_easy_perform(curl);
    long code = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &code);
    curl_easy_cleanup(curl);
    if (status != CURLE_OK || code != 200)
      throw std::runtime_error(
          "DomeD capture API unavailable; run the matching compositor build");
    auto j = Json::parse(body);
    if (j.at("version") != 1 || !j.value("available", false))
      throw std::runtime_error(
          j.value("reason", std::string("XR capture unavailable")));
    if (j.at("surfaces").size() > 64)
      throw std::runtime_error("XR capture is limited to 64 surfaces");
    return j;
  }
  EGLImageKHR image(int fd, int w, int h, uint32_t fourcc, uint32_t stride,
                    uint32_t offset, uint64_t modifier) {
    EGLint attributes[] = {EGL_WIDTH,
                           w,
                           EGL_HEIGHT,
                           h,
                           EGL_LINUX_DRM_FOURCC_EXT,
                           EGLint(fourcc),
                           EGL_DMA_BUF_PLANE0_FD_EXT,
                           fd,
                           EGL_DMA_BUF_PLANE0_OFFSET_EXT,
                           EGLint(offset),
                           EGL_DMA_BUF_PLANE0_PITCH_EXT,
                           EGLint(stride),
                           EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT,
                           EGLint(modifier & 0xffffffff),
                           EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT,
                           EGLint(modifier >> 32),
                           EGL_NONE};
    auto result = createImage(display, EGL_NO_CONTEXT, EGL_LINUX_DMA_BUF_EXT,
                              nullptr, attributes);
    if (result == EGL_NO_IMAGE_KHR)
      throw std::runtime_error("EGL DMA-BUF import failed on the selected GPU");
    return result;
  }
  GLuint texture(EGLImageKHR img) {
    GLuint t;
    glGenTextures(1, &t);
    glBindTexture(GL_TEXTURE_2D, t);
    imageTexture(GL_TEXTURE_2D, img);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    return t;
  }
  void remove(Texture &t) {
    if (t.name)
      glDeleteTextures(1, &t.name);
    if (t.image != EGL_NO_IMAGE_KHR)
      destroyImage(display, t.image);
    t = {}; // lease closes only after all GL work has finished.
  }
  Texture *surface(const std::string &socket, uint64_t id) {
    auto lease = receive(socket, id);
    if (!lease)
      return nullptr;
    struct stat st{};
    if (fstat(device->fd.value, &st) ||
        lease->info.at("device").get<uint64_t>() != uint64_t(st.st_rdev))
      throw std::runtime_error("XR surfaces are on a different GPU; choose the "
                               "compositor render node");
    auto &t = textures[id];
    if (t.lease && t.lease->info.at("serial") == lease->info.at("serial"))
      return &t;
    remove(t);
    auto &j = lease->info;
    t.image =
        image(lease->buffer.value, j.at("width"), j.at("height"),
              j.at("fourcc"), j.at("stride"), j.at("offset"), j.at("modifier"));
    t.name = texture(t.image);
    t.lease = std::move(lease);
    return &t;
  }
  void setup(const Json &j) {
    auto fov = j.at("viewport").get<std::array<float, 4>>();
    float left = std::tan(fov[0]), right = std::tan(fov[1]),
          bottom = std::tan(fov[2]), top = std::tan(fov[3]);
    if (!(right > left && top > bottom))
      throw std::runtime_error("Invalid XR capture field of view");
    width = options.width;
    height = std::max(64, int(width * (top - bottom) / (right - left)) / 2 * 2);
    if (height > 3840) {
      width = std::max(64, int(width * 3840. / height) / 2 * 2);
      height = 3840;
    }
    check(av_hwdevice_ctx_create(&drmDevice, AV_HWDEVICE_TYPE_DRM,
                                 options.device.c_str(), nullptr, 0),
          "Create DRM PRIME device");
    drmFrames = av_hwframe_ctx_alloc(drmDevice);
    if (!drmFrames)
      throw std::bad_alloc();
    auto frames = reinterpret_cast<AVHWFramesContext *>(drmFrames->data);
    frames->format = AV_PIX_FMT_DRM_PRIME;
    frames->sw_format = AV_PIX_FMT_BGRA;
    frames->width = width;
    frames->height = height;
    check(av_hwframe_ctx_init(drmFrames), "Create GPU frame context");
    for (int i = 0; i < 4; ++i)
      targets.push_back(std::make_shared<Target>(device, width, height));
    glGenRenderbuffers(1, &depth);
    glBindRenderbuffer(GL_RENDERBUFFER, depth);
    glRenderbufferStorage(GL_RENDERBUFFER, GL_DEPTH_COMPONENT16, width, height);
  }

public:
  explicit Eye(const Options &o)
      : options(o), device(std::make_shared<Device>(o.device)) {}
  void open() {
    auto platform = reinterpret_cast<PFNEGLGETPLATFORMDISPLAYEXTPROC>(
        eglGetProcAddress("eglGetPlatformDisplayEXT"));
    if (!platform)
      throw std::runtime_error("EGL GBM platform unavailable");
    display = platform(EGL_PLATFORM_GBM_KHR, device->gbm, nullptr);
    if (display == EGL_NO_DISPLAY || !eglInitialize(display, nullptr, nullptr))
      throw std::runtime_error("Initialize capture EGL");
    const char *extensions = eglQueryString(display, EGL_EXTENSIONS);
    if (!extensions ||
        !strstr(extensions, "EGL_EXT_image_dma_buf_import_modifiers"))
      throw std::runtime_error("EGL DMA-BUF modifiers unavailable");
    eglBindAPI(EGL_OPENGL_ES_API);
    EGLint attributes[] = {EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
                           EGL_SURFACE_TYPE, 0, EGL_NONE};
    EGLConfig config;
    EGLint count;
    if (!eglChooseConfig(display, attributes, &config, 1, &count) || !count)
      throw std::runtime_error("Choose EGL configuration");
    EGLint version[] = {EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE};
    context = eglCreateContext(display, config, EGL_NO_CONTEXT, version);
    if (context == EGL_NO_CONTEXT ||
        !eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, context))
      throw std::runtime_error("Create capture EGL context");
    createImage = reinterpret_cast<PFNEGLCREATEIMAGEKHRPROC>(
        eglGetProcAddress("eglCreateImageKHR"));
    destroyImage = reinterpret_cast<PFNEGLDESTROYIMAGEKHRPROC>(
        eglGetProcAddress("eglDestroyImageKHR"));
    imageTexture = reinterpret_cast<PFNGLEGLIMAGETARGETTEXTURE2DOESPROC>(
        eglGetProcAddress("glEGLImageTargetTexture2DOES"));
    if (!createImage || !destroyImage || !imageTexture)
      throw std::runtime_error("EGL image functions unavailable");
    auto shader = [](GLenum type, const char *source) {
      auto s = glCreateShader(type);
      glShaderSource(s, 1, &source, nullptr);
      glCompileShader(s);
      GLint ok;
      glGetShaderiv(s, GL_COMPILE_STATUS, &ok);
      if (!ok) {
        char log[1024];
        glGetShaderInfoLog(s, sizeof(log), nullptr, log);
        glDeleteShader(s);
        throw std::runtime_error(log);
      }
      return s;
    };
    const char *vertex =
        R"GLSL(attribute vec3 position; attribute vec2 uv; varying vec2 tex;
      uniform vec4 fov; void main() { float l=fov.x,r=fov.y,b=fov.z,t=fov.w;
      gl_Position=vec4(2.0*position.x/(r-l)+(r+l)*position.z/(r-l),
        2.0*position.y/(t-b)+(t+b)*position.z/(t-b),
        -1.0010005*position.z-0.10005003,-position.z); tex=uv; })GLSL";
    const char *fragment = R"GLSL(precision mediump float; varying vec2 tex;
      uniform sampler2D content; uniform float opacity;
      void main() { vec4 c=texture2D(content,tex)*opacity; if (c.a<0.003) discard; gl_FragColor=c; })GLSL";
    auto vs = shader(GL_VERTEX_SHADER, vertex),
         fs = shader(GL_FRAGMENT_SHADER, fragment);
    program = glCreateProgram();
    glAttachShader(program, vs);
    glAttachShader(program, fs);
    glBindAttribLocation(program, 0, "position");
    glBindAttribLocation(program, 1, "uv");
    glLinkProgram(program);
    glDeleteShader(vs);
    glDeleteShader(fs);
    GLint ok;
    glGetProgramiv(program, GL_LINK_STATUS, &ok);
    if (!ok)
      throw std::runtime_error("Link capture shaders");
    glGenBuffers(1, &vbo);
    glGenFramebuffers(1, &fbo);
  }
  Frame next() override {
    auto j = snapshot();
    if (!width)
      setup(j);
    std::shared_ptr<Target> target;
    for (auto &t : targets)
      if (t.use_count() == 1) {
        target = t;
        break;
      }
    if (!target)
      return {}; // bounded four-frame GPU pool, skip under load
    Fd fd(gbm_bo_get_fd(target->bo));
    if (fd.value < 0)
      throw std::runtime_error("Export capture target");
    auto img = image(fd.value, width, height, DRM_FORMAT_ARGB8888,
                     gbm_bo_get_stride(target->bo), 0,
                     gbm_bo_get_modifier(target->bo));
    auto tex = texture(img);
    struct Attachment {
      Eye *self;
      GLuint texture;
      EGLImageKHR image;
      ~Attachment() {
        glDeleteTextures(1, &texture);
        self->destroyImage(self->display, image);
      }
    } attachment{this, tex, img};
    glBindFramebuffer(GL_FRAMEBUFFER, fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D,
                           tex, 0);
    glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_DEPTH_ATTACHMENT,
                              GL_RENDERBUFFER, depth);
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE)
      throw std::runtime_error("Capture target is not renderable");
    glViewport(0, 0, width, height);
    glClearColor(0, 0, 0, 1);
    glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
    glEnable(GL_BLEND);
    glBlendFunc(GL_ONE, GL_ONE_MINUS_SRC_ALPHA);
    glEnable(GL_DEPTH_TEST);
    glDepthFunc(GL_LEQUAL);
    glUseProgram(program);
    glUniform1i(glGetUniformLocation(program, "content"), 0);
    auto fov = j.at("viewport").get<std::array<float, 4>>();
    for (auto &f : fov)
      f = std::tan(f);
    glUniform4fv(glGetUniformLocation(program, "fov"), 1, fov.data());
    Pose eye(j.at("eyes").at(options.eye));
    auto surfaces = j.at("surfaces").get<std::vector<Json>>();
    auto distance = [&](const Json &s) {
      Pose p(s.at("quad").at("pose"));
      auto d = p.position - eye.position;
      return d.x * d.x + d.y * d.y + d.z * d.z;
    };
    std::stable_sort(surfaces.begin(), surfaces.end(),
                     [&](const Json &a, const Json &b) {
                       return distance(a) > distance(b);
                     });
    std::vector<uint64_t> live;
    for (const auto &s : surfaces) {
      const auto &quad = s.at("quad");
      auto id = quad.at("id").get<uint64_t>();
      live.push_back(id);
      auto t = surface(j.at("gpu_socket"), id);
      if (!t)
        continue;
      auto &info = t->lease->info;
      bool curved = !quad.at("dome").is_null();
      Pose pose(curved ? quad.at("dome").at("frame") : quad.at("pose"));
      auto size = quad.at("size").get<std::array<float, 2>>();
      float radius = 0, angle = 0, top = 0, bottom = 0;
      if (curved) {
        auto &d = quad.at("dome");
        radius = d.at("radius");
        angle = d.at("horizontal");
        top = d.at("top");
        bottom = d.at("bottom");
      }
      unsigned steps =
          curved
              ? std::clamp(unsigned(std::ceil(std::abs(angle) * 32)), 4u, 128u)
              : 1u;
      std::vector<float> vertices;
      vertices.reserve(steps * 30);
      for (unsigned n = 0; n < steps; ++n)
        for (auto corner : std::array<std::array<int, 2>, 6>{
                 {{0, 0}, {1, 0}, {0, 1}, {0, 1}, {1, 0}, {1, 1}}}) {
          float u = float(n + corner[0]) / steps, v = float(corner[1]);
          Vec p = curved ? Vec{radius * std::sin((u - .5f) * angle),
                               radius * (top + (bottom - top) * v),
                               -radius * std::cos((u - .5f) * angle)}
                         : Vec{(u - .5f) * size[0], (.5f - v) * size[1], 0};
          p = eye.view(pose.world(p));
          float sampleU = u, sampleV = v;
          if (s.value("stereo", false)) {
            auto sourceWidth = info.at("width").get<unsigned>();
            unsigned half = sourceWidth / 2;
            unsigned start = options.eye ? sourceWidth - half : 0;
            sampleU = (start + u * half) / sourceWidth;
          }
          if (info.contains("viewport") && !info["viewport"].is_null()) {
            auto &vp = info["viewport"];
            float logicalHeight = info.at("logical_height");
            sampleV = (vp.at("offset").get<float>() +
                       v * vp.at("height").get<float>()) /
                      logicalHeight;
          }
          if (info.value("y_inverted", false))
            sampleV = 1 - sampleV;
          // GL's framebuffer row zero is the bottom. VA-API consumes top-down
          // DRM rows, so render vertically inverted, including the projection.
          vertices.insert(vertices.end(), {p.x, -p.y, p.z, sampleU, sampleV});
        }
      // Swap the vertical FOV too when writing top-down capture rows.
      std::array<float, 4> flipped{fov[0], fov[1], -fov[3], -fov[2]};
      glUniform4fv(glGetUniformLocation(program, "fov"), 1, flipped.data());
      glUniform1f(glGetUniformLocation(program, "opacity"),
                  s.value("opacity", 1.f));
      glActiveTexture(GL_TEXTURE0);
      glBindTexture(GL_TEXTURE_2D, t->name);
      glBindBuffer(GL_ARRAY_BUFFER, vbo);
      glBufferData(GL_ARRAY_BUFFER, vertices.size() * sizeof(float),
                   vertices.data(), GL_STREAM_DRAW);
      glEnableVertexAttribArray(0);
      glVertexAttribPointer(0, 3, GL_FLOAT, GL_FALSE, 5 * sizeof(float),
                            nullptr);
      glEnableVertexAttribArray(1);
      glVertexAttribPointer(1, 2, GL_FLOAT, GL_FALSE, 5 * sizeof(float),
                            reinterpret_cast<void *>(3 * sizeof(float)));
      glDrawArrays(GL_TRIANGLES, 0, vertices.size() / 5);
    }
    // Only the recording worker waits. Headset submission and desktop scanout
    // remain independent. This also makes producer leases safe to release.
    glFinish();
    if (glGetError() != GL_NO_ERROR)
      throw std::runtime_error("GPU eye render failed");
    for (auto i = textures.begin(); i != textures.end();) {
      if (std::find(live.begin(), live.end(), i->first) == live.end()) {
        remove(i->second);
        i = textures.erase(i);
      } else
        ++i;
    }
    auto f = frame();
    f->format = AV_PIX_FMT_DRM_PRIME;
    f->width = width;
    f->height = height;
    auto payload = std::make_unique<Payload>();
    payload->target = target;
    auto &d = payload->descriptor;
    d.nb_objects = 1;
    d.objects[0].fd = fd.value;
    fd.value = -1;
    d.objects[0].size = uint64_t(gbm_bo_get_stride(target->bo)) * height;
    d.objects[0].format_modifier = gbm_bo_get_modifier(target->bo);
    d.nb_layers = 1;
    d.layers[0].format = DRM_FORMAT_ARGB8888;
    d.layers[0].nb_planes = 1;
    d.layers[0].planes[0].object_index = 0;
    d.layers[0].planes[0].offset = 0;
    d.layers[0].planes[0].pitch = gbm_bo_get_stride(target->bo);
    f->buf[0] = av_buffer_create(
        reinterpret_cast<uint8_t *>(payload.get()), sizeof(Payload),
        +[](void *, uint8_t *data) {
          delete reinterpret_cast<Payload *>(data);
        },
        nullptr, 0);
    if (!f->buf[0])
      throw std::bad_alloc();
    f->data[0] = reinterpret_cast<uint8_t *>(&payload.release()->descriptor);
    f->hw_frames_ctx = av_buffer_ref(drmFrames);
    return f;
  }
  std::string transport() const override {
    return "DMA-BUF → EGL eye render → DRM PRIME → VA-API (no pixel readback)";
  }
  ~Eye() override {
    if (context != EGL_NO_CONTEXT) {
      eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, context);
      glFinish();
      for (auto &p : textures)
        remove(p.second);
      if (program)
        glDeleteProgram(program);
      if (vbo)
        glDeleteBuffers(1, &vbo);
      if (fbo)
        glDeleteFramebuffers(1, &fbo);
      if (depth)
        glDeleteRenderbuffers(1, &depth);
      eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
      eglDestroyContext(display, context);
    }
    av_buffer_unref(&drmFrames);
    av_buffer_unref(&drmDevice);
    if (display != EGL_NO_DISPLAY)
      eglTerminate(display);
  }
};
} // namespace
std::unique_ptr<Source> makeEyeSource(const Options &o) {
  auto source = std::make_unique<Eye>(o);
  source->open();
  return source;
}
} // namespace capture
