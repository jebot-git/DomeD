// SPDX-License-Identifier: MIT
// Run on private Xvfb with FONT_DIRECTORY OUTPUT_DIRECTORY SCALE. Define GPU_PROBE for the GL widget path.
#ifndef APPEARANCE_HEADER
#define APPEARANCE_HEADER "../native/fltk_appearance.h"
#endif
#include APPEARANCE_HEADER
#include <FL/Fl_Double_Window.H>
#include <ft2build.h>
#include FT_FREETYPE_H
#include <iostream>
#include <stdexcept>
#ifdef GPU_PROBE
#include "../native/gpu-window.h"
struct GlyphWindow : dx::GpuWindow {
  std::vector<unsigned char> pixels = std::vector<unsigned char>(384 * 320 * 3);
  double measured = 0;
  GlyphWindow() : dx::GpuWindow(384, 320) { end(); }
  void draw() override {
    dx::GpuWindow::draw();
    draw_begin();
    fl_color(FL_BLACK); fl_rectf(0, 0, 384, 320);
    fl_font(dx::font, dx::size); measured = fl_width("MMMM", 4);
    fl_color(FL_WHITE); fl_draw("M", 32, 260);
    draw_end();
    glPixelStorei(GL_PACK_ALIGNMENT, 1);
    glReadPixels(0, 0, 384, 320, GL_RGB, GL_UNSIGNED_BYTE, pixels.data());
  }
};
#endif

int main(int argc, char **argv) try {
  if (argc != 4) return 2;
  int scale = std::stoi(argv[3]);
  if (scale != 1 && scale != 2) return 2;
  auto output = std::filesystem::path(argv[2]);
  std::filesystem::create_directories(output / "config/dome");
  setenv("XDG_CONFIG_HOME", (output / "config").c_str(), 1);
  setenv("XDG_CURRENT_DESKTOP", "Dome", 1);
  FT_Library library;
  if (FT_Init_FreeType(&library)) return 2;
  std::vector<std::filesystem::path> paths;
  for (auto &entry : std::filesystem::recursive_directory_iterator(argv[1]))
    if (entry.path().extension() == ".bdf" && entry.path().filename() != "upstream.bdf") {
      paths.push_back(entry.path());
      if (!FcConfigAppFontAddFile(nullptr, (FcChar8 *)entry.path().c_str()))
        throw std::runtime_error("Fontconfig rejected BDF " + entry.path().string());
    }
  FcConfigBuildFonts(nullptr);
  Fl_Double_Window window(320, 100);
  window.end(); window.show(); Fl::check();
#ifdef GPU_PROBE
  GlyphWindow gl;
  gl.show();
#endif
  int failures = 0;
  for (auto &path : paths) {
    FT_Face face = nullptr;
    if (FT_New_Face(library, path.c_str(), 0, &face) || FT_IS_SCALABLE(face) ||
        FT_Select_Charmap(face, FT_ENCODING_UNICODE) || FT_Select_Size(face, 0))
      throw std::runtime_error("Cannot load native BDF " + path.string());
    if (!FT_Get_Char_Index(face, 'M') || !FT_Get_Char_Index(face, 'A')) { FT_Done_Face(face); continue; }
    int pixels = face->available_sizes[0].y_ppem / 64;
    std::string id = bdf_fonts::identity(face->family_name, face->style_name);
    std::ofstream(dx::config()) << Json{{"font", id}, {"font_size", pixels}, {"font_scale", scale}, {"light", false}, {"accent", "#6babff"}};
    if (scale == 2) {
      bdf_fonts::Face base; bdf_fonts::inspect(path.string(), base);
      auto enlarged = bdf_fonts::scaled(base, scale);
      FT_Done_Face(face);
      if (FT_New_Face(library, enlarged.file.c_str(), 0, &face) || FT_Select_Size(face, 0)) return 2;
      pixels *= 2; id = enlarged.id();
    }
    std::string expected_name = std::string(face->family_name) + " / " + face->style_name;
    dx::appearance();
    Fl_Image_Surface surface(384, 320);
    Fl_Surface_Device::push_current(&surface);
    fl_color(FL_BLACK); fl_rectf(0, 0, 384, 320);
    fl_font(dx::font, dx::size);
    double measured = fl_width("MMMM", 4);
    fl_color(FL_WHITE); fl_draw("M", 32, 260);
    auto image = surface.image();
    Fl_Surface_Device::pop_current();
#ifdef GPU_PROBE
    gl.redraw();
    for (int i = 0; i < 3; ++i) Fl::wait(.02);
    measured = gl.measured;
    delete image;
    std::vector<unsigned char> upright(gl.pixels.size());
    for (int y = 0; y < 320; ++y)
      std::copy_n(gl.pixels.data() + (319 - y) * 1152, 1152, upright.data() + y * 1152);
    image = new Fl_RGB_Image(upright.data(), 384, 320, 3);
#endif
    FT_Load_Char(face, 'M', FT_LOAD_RENDER | FT_LOAD_TARGET_MONO | FT_LOAD_MONOCHROME);
    auto &bitmap = face->glyph->bitmap;
    std::vector<std::pair<int, int>> expected, actual;
    for (unsigned y = 0; y < bitmap.rows; ++y)
      for (unsigned x = 0; x < bitmap.width; ++x)
        if (bitmap.buffer[y * bitmap.pitch + x / 8] & (0x80 >> (x % 8)))
          expected.emplace_back(x, y);
    int stride = image->ld() ? image->ld() : image->w() * image->d();
    auto data = (const unsigned char *)image->data()[0];
    bool monochrome = true;
    for (int y = 0; y < image->h(); ++y) for (int x = 0; x < image->w(); ++x) {
      unsigned v = data[y * stride + x * image->d()];
      if (v) actual.emplace_back(x, y);
      if (v != 0 && v != 255) monochrome = false;
    }
    auto normalize = [](auto &points) {
      if (points.empty()) return;
      int x = points.front().first, y = points.front().second;
      for (auto &point : points) { point.first -= x; point.second -= y; }
    };
    normalize(expected); normalize(actual);
    bool ok = dx::font == FL_FREE_FONT && dx::size == pixels &&
      dx::font_name == expected_name && measured == 4 * (face->glyph->advance.x / 64.) &&
      !actual.empty() && expected == actual && monochrome;
    std::cout << (ok ? "PASS " : "FAIL ") << id << " " << pixels
              << "px: width=" << measured << "/" << 4 * (face->glyph->advance.x / 64.)
              << " mask=" << (expected == actual) << " mono=" << monochrome
              << " chosen=" << dx::font_name << "\n";
    failures += !ok;
    delete image; FT_Done_Face(face);
  }
  std::cout << paths.size() << " BDF files, " << failures << " failures\n";
  window.hide(); FT_Done_FreeType(library);
  return failures ? 1 : 0;
} catch (const std::exception &e) { std::cerr << e.what() << "\n"; return 2; }
