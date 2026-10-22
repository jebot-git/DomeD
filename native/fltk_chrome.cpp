// SPDX-License-Identifier: MIT
// FLTK experiment: headless shell rasterization uses Cairo/FreeType. It must
// never open a toolkit connection to the Wayland server owned by this process.
// Legacy C ABI names preserve the Rust-owned compositor/input boundary.
#include "suite-icons.h"
#include "keyboard-flags.h"
#include "theme_icons.hpp"
#include <FL/Fl_Shared_Image.H>
#include <FL/Fl_PNG_Image.H>
#include <FL/Fl_Pixmap.H>
#include <FL/Fl_SVG_Image.H>
#include <cairo.h>
#include <fontconfig/fontconfig.h>
#include <ft2build.h>
#include <glib.h>
#include <memory>
#include FT_FREETYPE_H
#include FT_FONT_FORMATS_H
#include "bdf_font.hpp"
#include <algorithm>
#include <cmath>
#include <cstring>
#include <ctime>
#include <filesystem>
#include <map>
#include <nlohmann/json.hpp>
#include <set>
#include <string>
#include <vector>
using J = nlohmann::json;
namespace {
FT_Library library = nullptr;
struct Font {
  FT_Face face;
  std::string id;
  std::vector<int> sizes;
  std::string file;
};
std::vector<Font> fonts;
std::vector<std::vector<unsigned char>> bundled;
std::vector<bdf_fonts::Face> available;
void add(FT_Face face, const std::string &file = "") {
  if (!FT_Get_Font_Format(face) || std::strcmp(FT_Get_Font_Format(face), "BDF") || FT_IS_SCALABLE(face) || !face->num_fixed_sizes ||
      FT_Select_Charmap(face, FT_ENCODING_UNICODE)) {
    FT_Done_Face(face);
    return;
  }
  std::vector<int> sizes;
  for (int i = 0; i < face->num_fixed_sizes; ++i) {
    int n = std::lround(face->available_sizes[i].y_ppem / 64.);
    if (n >= 4 && n <= 64)
      sizes.push_back(n);
  }
  if (sizes.empty()) {
    FT_Done_Face(face);
    return;
  }
  std::sort(sizes.begin(), sizes.end());
  sizes.erase(std::unique(sizes.begin(), sizes.end()), sizes.end());
  fonts.push_back(
      {face,
       bdf_fonts::identity(face->family_name ? face->family_name : "Bitmap",
           face->style_name ? face->style_name : "Regular"),
       sizes, file});
}
void ensureFont(const std::string &id, int pixels) {
  for (const auto &font : fonts)
    if (font.id == id && std::find(font.sizes.begin(),font.sizes.end(),pixels)!=font.sizes.end()) return;
  for (const auto &font : available) {
    if (font.id()!=id || font.pixels!=pixels) continue;
    FT_Face face;
    if (!FT_New_Face(library,font.file.c_str(),0,&face)) add(face,font.file);
    return;
  }
}
FT_Face scaledFace(FT_Face face, int scale) {
  if (scale == 1) return face;
  struct Enlarged { std::string data; FT_Face face = nullptr; };
  static std::map<FT_Face, Enlarged> cache;
  auto &entry = cache[face];
  if (!entry.face) {
    std::string source;
    if (face->stream->base) source.assign((char *)face->stream->base, face->stream->size);
    else {
      auto found = std::find_if(fonts.begin(), fonts.end(), [face](const Font &f) { return f.face == face; });
      if (found == fonts.end()) throw std::runtime_error("Missing BDF source");
      source = bdf_fonts::read(found->file);
    }
    entry.data = bdf_fonts::twice(source, face->family_name);
    if (FT_New_Memory_Face(library, (const FT_Byte *)entry.data.data(), entry.data.size(), 0, &entry.face) ||
        FT_Select_Charmap(entry.face, FT_ENCODING_UNICODE) || FT_Select_Size(entry.face, 0))
      throw std::runtime_error("Cannot load 2x BDF");
  }
  return entry.face;
}
std::string str(const J &o, const char *k, std::string d = "") {
  return o.contains(k) && o[k].is_string() ? o[k].get<std::string>() : d;
}
int num(const J &o, const char *k, int d = 0) {
  return o.contains(k) && o[k].is_number() ? o[k].get<int>() : d;
}
bool yes(const J &o, const char *k, bool d = false) {
  return o.contains(k) && o[k].is_boolean() ? o[k].get<bool>() : d;
}
J obj(const J &o, const char *k) {
  return o.contains(k) && o[k].is_object() ? o[k] : J::object();
}
J arr(const J &o, const char *k) {
  return o.contains(k) && o[k].is_array() ? o[k] : J::array();
}
struct Color {
  double r, g, b, a = 1;
};
Color hex(std::string s, Color fallback) {
  try {
    if (s.size() == 7 && s[0] == '#') {
      auto n = std::stoul(s.substr(1), nullptr, 16);
      return {double(n >> 16) / 255, double((n >> 8) & 255) / 255,
              double(n & 255) / 255, 1};
    }
  } catch (...) {
  }
  return fallback;
}
struct Paint {
  cairo_t *c;
  FT_Face face = nullptr;
  int size = 16;
  int scale = 1;
  Color bg, fg, muted, sel, accent;
  double opacity = 1;
  Paint(cairo_t *cr, const J &a) : c(cr) {
    bool light = yes(a, "light");
    bg = light ? Color{.90, .91, .94} : Color{.12, .145, .176};
    fg = light ? Color{.12, .145, .18} : Color{.906, .922, .945};
    muted = light ? Color{.41, .45, .52} : Color{.57, .60, .65};
    sel = light ? Color{.78, .84, .91} : Color{.247, .302, .376};
    accent = hex(str(a, "accent", "#6babff"), {.42, .67, 1});
    opacity = std::clamp(num(a, "opacity", 255), 0, 255) / 255.;
    auto id = str(a, "font", "Unifont / Sans-Serif");
    size = num(a, "font_size", 16);
    scale = num(a, "font_scale", 1) == 2 ? 2 : 1;
    ensureFont(id, size);
    for (auto &f : fonts)
      if (f.id == id && std::find(f.sizes.begin(), f.sizes.end(), size) != f.sizes.end()) {
        face = f.face;
        break;
      }
    if (!face && !fonts.empty())
      face = fonts[0].face;
    if (face) {
      bool found = false;
      for (int i = 0; i < face->num_fixed_sizes; ++i)
        if (int(face->available_sizes[i].y_ppem / 64) == size) {
          FT_Select_Size(face, i);
          found = true;
          break;
        }
      if (!found) {
        FT_Select_Size(face, 0);
        size = face->available_sizes[0].y_ppem / 64;
      }
      face = scaledFace(face, scale);
      size *= scale;
    }
  }
  void color(Color a) { cairo_set_source_rgba(c, a.r, a.g, a.b, a.a); }
  void rect(int x, int y, int w, int h, Color a) {
    if (w <= 0 || h <= 0)
      return;
    color(a);
    cairo_rectangle(c, x, y, w, h);
    cairo_fill(c);
  }
  void border(int x, int y, int w, int h, Color a) {
    color(a);
    cairo_set_line_width(c, 1);
    cairo_rectangle(c, x + .5, y + .5, w - 1, h - 1);
    cairo_stroke(c);
  }
  void text(int x, int y, int w, int h, const std::string &s, Color a,
            bool center = false) {
    if (w <= 0 || h <= 0)
      return;
    cairo_save(c);
    cairo_rectangle(c, x, y, w, h);
    cairo_clip(c);
    color(a);
    double baseline = y + (h + size) / 2 - 2;
    int cursor = x + (center ? 8 : 0);
    if (!face) {
      cairo_restore(c);
      return;
    }
    const unsigned char *p = (const unsigned char *)s.data();
    size_t i = 0;
    while (i < s.size()) {
      unsigned cp = p[i++];
      if (cp >= 0xc0) {
        int more = (cp & 0xe0) == 0xc0   ? 1
                   : (cp & 0xf0) == 0xe0 ? 2
                   : (cp & 0xf8) == 0xf0 ? 3
                                         : 0;
        cp &= (1u << (6 - more)) - 1;
        while (more-- && i < s.size())
          cp = (cp << 6) | (p[i++] & 63);
      }
      if (cp == '\n') {
        baseline += size + 3;
        cursor = x;
        continue;
      }
      FT_Face glyph_face = face;
      if (!FT_Get_Char_Index(glyph_face, cp)) {
        for (auto &fallback : fonts) {
          if (!FT_Get_Char_Index(fallback.face, cp)) continue;
          glyph_face = scaledFace(fallback.face, scale);
          int strike = 0, distance = 1000000;
          for (int n = 0; n < glyph_face->num_fixed_sizes; ++n) {
            int diff = std::abs(int(glyph_face->available_sizes[n].y_ppem / 64) - size);
            if (diff < distance) { strike = n; distance = diff; }
          }
          FT_Select_Size(glyph_face, strike);
          break;
        }
      }
      if (FT_Load_Char(glyph_face, cp,
                       FT_LOAD_RENDER | FT_LOAD_MONOCHROME |
                           FT_LOAD_TARGET_MONO))
        continue;
      auto &b = glyph_face->glyph->bitmap;
      if (b.width && b.rows) {
        int stride = cairo_format_stride_for_width(CAIRO_FORMAT_A8, b.width);
        std::vector<unsigned char> data(stride * b.rows);
        for (unsigned row = 0; row < b.rows; ++row)
          for (unsigned col = 0; col < b.width; ++col) {
            auto src = b.buffer + (b.pitch < 0 ? b.rows - 1 - row : row) *
                                      std::abs(b.pitch);
            data[row * stride + col] =
                b.pixel_mode == FT_PIXEL_MODE_MONO
                    ? ((src[col / 8] & (0x80 >> (col % 8))) ? 255 : 0)
                    : src[col];
          }
        auto surface = cairo_image_surface_create_for_data(
            data.data(), CAIRO_FORMAT_A8, b.width, b.rows, stride);
        cairo_mask_surface(c, surface, cursor + glyph_face->glyph->bitmap_left,
                           baseline - glyph_face->glyph->bitmap_top);
        cairo_surface_destroy(surface);
      }
      cursor += glyph_face->glyph->advance.x / 64;
      if (cursor > x + w)
        break;
    }
    cairo_restore(c);
  }
  bool image(int x, int y, int w, int h, const J &object, const char *key,
             bool argb = false) {
    int iw = num(object, "width"), ih = num(object, "height");
    if (iw < 1 || ih < 1 || iw > 2048 || ih > 2048)
      return false;
    auto encoded = str(object, key);
    gsize length = 0;
    auto data = g_base64_decode(encoded.c_str(), &length);
    if (length != size_t(iw) * ih * 4) {
      g_free(data);
      return false;
    }
    std::vector<uint32_t> pixels(size_t(iw) * ih);
    for (size_t n = 0; n < pixels.size(); ++n) {
      auto p = data + n * 4;
      unsigned a = p[argb ? 0 : 3], r = p[argb ? 1 : 0], g = p[argb ? 2 : 1],
               b = p[argb ? 3 : 2];
      pixels[n] = (a << 24) | ((r * a / 255) << 16) | ((g * a / 255) << 8) |
                  (b * a / 255);
    }
    g_free(data);
    auto surface = cairo_image_surface_create_for_data(
        (unsigned char *)pixels.data(), CAIRO_FORMAT_ARGB32, iw, ih, iw * 4);
    double scale = std::min(double(w) / iw, double(h) / ih);
    cairo_save(c);
    cairo_translate(c, x + (w - iw * scale) / 2, y + (h - ih * scale) / 2);
    cairo_scale(c, scale, scale);
    cairo_set_source_surface(c, surface, 0, 0);
    cairo_paint(c);
    cairo_restore(c);
    cairo_surface_destroy(surface);
    return true;
  }
  void icon(int x, int y, int extent, std::string name, Color tint,
            const std::string &theme_path = "", bool themed = true) {
    char color[8];
    snprintf(color, sizeof(color), "#%02x%02x%02x", int(tint.r * 255),
             int(tint.g * 255), int(tint.b * 255));
    bool flag = name.rfind("flag-", 0) == 0;
    auto role = flag ? name : dx_icon::role(name);
    auto key = (themed ? name + theme_path : role) + color + std::to_string(extent);
    struct Raster {
      std::vector<uint32_t> pixels;
      int w = 0, h = 0;
    };
    static std::map<std::string, Raster> cache;
    auto it = cache.find(key);
    if (it == cache.end()) {
      auto svg = flag ? dx_flags::svg(name.substr(5), color) : dx_icon::svg(role, color);
      std::unique_ptr<Fl_Image> source;
      if (themed && !flag) {
        auto path = theme_icons::resolve(name, theme_path, extent);
        if (!path.empty()) {
          auto shared = Fl_Shared_Image::get(path.c_str());
          if (shared) {
            if (auto pixmap = dynamic_cast<const Fl_Pixmap *>(shared->image())) {
              Fl_RGB_Image rgb(pixmap);
              source.reset(rgb.copy(extent, extent));
            } else source.reset(shared->image()->copy(extent, extent));
            shared->release();
          }
        }
      }
      if (!source || source->fail()) source = std::make_unique<Fl_SVG_Image>(nullptr, svg.c_str());
      auto &image = *source;
      if (auto vector = dynamic_cast<Fl_SVG_Image *>(&image)) vector->resize(extent, extent);
      if (auto rgb = dynamic_cast<Fl_RGB_Image *>(&image)) rgb->normalize();
      if (image.fail() || !image.data() || !image.data()[0])
        return;
      Raster raster;
      raster.w = image.data_w();
      raster.h = image.data_h();
      raster.pixels.resize(size_t(raster.w) * raster.h);
      auto data = (const unsigned char *)image.data()[0];
      int stride = image.ld() ? image.ld() : raster.w * image.d();
      for (int row = 0; row < raster.h; ++row)
        for (int col = 0; col < raster.w; ++col) {
          auto p = data + row * stride + col * image.d();
          unsigned a = image.d() == 4 ? p[3] : image.d() == 2 ? p[1] : 255;
          unsigned r = p[0], g = image.d() < 3 ? p[0] : p[1], b = image.d() < 3 ? p[0] : p[2];
          raster.pixels[row * raster.w + col] =
              (a << 24) | ((r * a / 255) << 16) | ((g * a / 255) << 8) |
              (b * a / 255);
        }
      if (cache.size() > 256)
        cache.clear();
      it = cache.emplace(key, std::move(raster)).first;
    }
    auto &r = it->second;
    auto surface = cairo_image_surface_create_for_data(
        (unsigned char *)r.pixels.data(), CAIRO_FORMAT_ARGB32, r.w, r.h,
        r.w * 4);
    cairo_save(c);
    cairo_set_source_surface(c, surface, x, y);
    cairo_paint_with_alpha(c, tint.a);
    cairo_restore(c);
    cairo_surface_destroy(surface);
  }
  void taskPopup(const J &panel) {
    if (panel.empty())
      return;
    int x = num(panel, "x"), y = num(panel, "y"), w = num(panel, "w"),
        h = num(panel, "h"), band = num(panel, "scroll_band"),
        offset = num(panel, "offset");
    rect(x, y, w, h, bg);
    border(x, y, w, h, muted);
    cairo_save(c);
    cairo_rectangle(c, x, y + band, w, std::max(0, h - 2 * band));
    cairo_clip(c);
    bool context = yes(panel, "context");
    int i = 0;
    for (auto &row : arr(panel, "rows")) {
      int index = i++, ry = y + band + 6 + index * (context ? 32 : 88) - offset;
      if (ry + 88 < y + band || ry >= y + h - band)
        continue;
      if (index == num(panel, "selected", -1) ||
          (!context && yes(row, "active")))
        rect(x + 4, ry, w - 8, context ? 32 : 84, sel);
      if (context)
        text(x + 14, ry, w - 28, 32, str(row, "text"),
             yes(row, "enabled", true) ? fg : muted);
      else {
        if (!image(x + 10, ry + 4, 126, 76, row, "rgba"))
          rect(x + 10, ry + 4, 126, 76, sel);
        text(x + 146, ry + 5, w - 158, 70, str(row, "text"),
             yes(row, "minimized") ? muted : fg);
      }
    }
    cairo_restore(c);
    if (band)
      for (bool up : {true, false}) {
        int ry = up ? y : y + h - band;
        rect(x, ry, w, band, bg);
        text(x, ry, w, band, up ? "▲" : "▼",
             yes(panel, up ? "can_up" : "can_down") ? fg : muted, true);
      }
  }
  void button(int x, int y, int w, int h, const std::string &label,
              bool selected = false) {
    rect(x, y, w, h, sel);
    if (selected)
      border(x, y, w, h, accent);
    text(x + 8, y, w - 16, h, label, fg);
  }
  // Hit IDs match dome_qt_hit and Rust's release actions: close, zoom, depth.
  // Draw the Workbench gadgets directly; their meaning must not depend on fonts
  // or the generic minimize/maximize/close icons supplied by an icon theme.
  void workbenchGadget(int x, int hit, Color caption, bool active, bool down,
                       bool zoomed, int height) {
    cairo_save(c);
    cairo_set_antialias(c, CAIRO_ANTIALIAS_NONE);
    auto fill = down ? sel : caption;
    auto ink = active ? fg : muted;
    auto shine = caption, shadow = caption;
    for (auto channel : {&shine.r, &shine.g, &shine.b})
      *channel += (1 - *channel) * .35;
    for (auto channel : {&shadow.r, &shadow.g, &shadow.b})
      *channel *= .55;
    rect(x, 1, 28, height - 2, fill);
    auto top = down ? shadow : shine, bottom = down ? shine : shadow;
    rect(x, 1, 28, 1, top); rect(x, 1, 1, height - 2, top);
    rect(x, height - 2, 28, 1, bottom); rect(x + 27, 1, 1, height - 2, bottom);
    int cx = x + 13 + int(down), cy = height / 2 - 1 + int(down);
    if (hit == 1) {
      rect(cx - 4, cy - 4, 9, 9, ink);
    } else if (hit == 3) {
      // Two remembered size/position presets, rather than maximization.
      border(cx - 7, cy - 5, 15, 11, ink);
      if (zoomed) rect(cx - 5, cy - 3, 6, 5, ink);
      else border(cx - 5, cy - 3, 6, 5, ink);
    } else if (hit == 6) {
      // The foreground plane occludes the rear plane: change stacking depth.
      border(cx - 7, cy - 5, 11, 9, ink);
      rect(cx - 2, cy - 1, 11, 9, fill);
      border(cx - 2, cy - 1, 11, 9, ink);
    }
    cairo_restore(c);
  }
  void panelRows(const J &panel, const std::string &title, int rowHeight,
                 int top = 40) {
    if (panel.empty())
      return;
    int x = num(panel, "x"), y = num(panel, "y"),
        w = num(panel, "w", num(panel, "width", 600)), h = num(panel, "h", 400);
    rect(x, y, w, h, bg);
    text(x + 16, y + 4, w - 32, 32, title, fg);
    auto rows = arr(panel, "rows");
    int first = num(panel, "first"),
        visible = num(panel, "visible", rows.size());
    for (int i = first; i < std::min(int(rows.size()), first + visible); ++i) {
      int ry = y + top + (i - first) * rowHeight;
      auto r = rows[i];
      if (i == num(panel, "selected", -1))
        rect(x + 10, ry, w - 20, rowHeight, sel);
      text(x + 18, ry, w * 2 / 3 - 20, rowHeight,
           str(r, "label", str(r, "text")), fg);
      text(x + w * 2 / 3, ry, w / 3 - 18, rowHeight,
           str(r, "value", str(r, "detail")), muted);
    }
    text(x + 16, y + h - 30, w - 32, 26, str(panel, "error"), accent);
  }
  void menu(const J &rows, int x, int y, int w, int selected) {
    rect(x, y, w, int(rows.size()) * 28 + 8, bg);
    border(x, y, w, int(rows.size()) * 28 + 8, muted);
    int i = 0;
    for (auto &row : rows) {
      int ry = y + 4 + i * 28;
      if (yes(row, "separator")) {
        rect(x + 12, ry + 14, w - 24, 1, muted);
        ++i;
        continue;
      }
      if (i == selected && yes(row, "enabled", true))
        rect(x + 3, ry, w - 6, 28, sel);
      auto color = yes(row, "enabled", true) ? fg : muted;
      if (yes(row, "checked"))
        text(x + 6, ry, 22, 28, yes(row, "radio") ? "●" : "✓", color);
      int reserve = str(row, "shortcut").empty() ? 28 : 164;
      bool flag = str(row, "icon").rfind("flag-", 0) == 0;
      if (flag) icon(x + 29, ry + 2, 24, str(row, "icon"), color, "", false);
      int inset = flag ? 60 : 32;
      text(x + inset, ry, std::max(0, w - inset - reserve), 28, str(row, "text"),
           color);
      text(x + w - 150, ry, 125, 28, str(row, "shortcut"), color);
      if (yes(row, "submenu"))
        text(x + w - 25, ry, 20, 28, "▸", color);
      ++i;
    }
  }
};
} // namespace
extern "C" void dome_qt_queue_bitmap_font(const unsigned char *data,
                                          int length) {
  if (data && length > 0)
    bundled.emplace_back(data, data + length);
}
extern "C" const char *dome_qt_font_catalog(const unsigned char *data,
                                            int length) {
  static std::string result;
  static bool initialized = false;
  if (!initialized) {
    if (FT_Init_FreeType(&library)) { result = "[]"; return result.c_str(); }
    dome_qt_queue_bitmap_font(data, length);
    for (auto &bytes : bundled) {
      FT_Face face;
      if (!FT_New_Memory_Face(library, bytes.data(), bytes.size(), 0, &face)) add(face);
    }
    initialized = true;
  }
  available = bdf_fonts::discover();
  J catalog = J::array();
  std::map<std::string, size_t> indices;
  for (const auto &font : available) {
    if (!font.text || font.pixels > 64) continue;
    auto found = indices.find(font.id());
    if (found == indices.end()) {
      indices[font.id()] = catalog.size();
      catalog.push_back({{"id",font.id()},{"family",font.family},{"style",font.style},{"file",font.file},{"sizes",J::array({font.pixels})}});
    } else {
      auto sizes=catalog[found->second]["sizes"].get<std::vector<int>>();
      sizes.push_back(font.pixels);std::sort(sizes.begin(),sizes.end());sizes.erase(std::unique(sizes.begin(),sizes.end()),sizes.end());
      catalog[found->second]["sizes"]=sizes;
    }
  }
  // Unifont supplies fallback coverage; interface icons are vector assets.
  for (const auto &font : available)
    if (font.family=="Unifont") ensureFont(font.id(),font.pixels);
  result = catalog.dump();
  return result.c_str();
}
extern "C" int dome_qt_hit(int w, int h, int x, int y, int caption = 30) {
  if (x < 0 || y < 0 || x >= w || y >= h)
    return 0;
  if (y < caption) {
    if (x < 30)
      return 1;
    if (x >= w - 60 && x < w - 30)
      return 3;
    if (x >= w - 30)
      return 6;
    return 4;
  }
  if (x < 4 || x >= w - 4 || y >= h - 4)
    return 5;
  return 0;
}
extern "C" int dome_qt_paint(const char *json, unsigned char *pixels,
                             int length) try {
  J o = J::parse(json);
  int w = num(o, "width"), h = num(o, "height");
  if (w < 1 || h < 1 || int64_t(w) * h * 4 != length)
    return 0;
  memset(pixels, 0, length);
  auto surface = cairo_image_surface_create_for_data(
      pixels, CAIRO_FORMAT_ARGB32, w, h, w * 4);
  auto c = cairo_create(surface);
  static const bool image_handlers = [] { fl_register_images(); return true; }();
  (void)image_handlers;
  Paint p(c, obj(o, "appearance"));
  if (str(o, "kind") == "frame") {
    int bar_height = std::max(30, num(obj(o, "appearance"), "font_size", 16) * num(obj(o, "appearance"), "font_scale", 1) + 8);
    bool active = yes(o, "active");
    auto caption = active ? p.sel : p.bg;
    cairo_push_group(c);
    p.rect(0, 0, w, bar_height, caption);
    p.text(38, 0, w - 106, bar_height, str(o, "title"), p.fg);
    for (int hit : {1, 3, 6}) {
      int x = hit == 1 ? 1 : hit == 3 ? w - 59 : w - 29;
      p.workbenchGadget(x, hit, caption, active, num(o, "pressed") == hit,
                        yes(o, "zoomed"), bar_height);
    }
    cairo_pop_group_to_source(c);
    cairo_paint_with_alpha(c, p.opacity);
    auto border = p.accent;
    border.a = .4 * p.opacity;
    p.rect(0, bar_height, 4, h - bar_height, border);
    p.rect(w - 4, bar_height, 4, h - bar_height, border);
    p.rect(0, h - 4, w, 4, border);
  } else if (str(o, "kind") != "notifications") {
    auto bar = p.bg;
    bar.a = p.opacity;
    p.rect(0, 0, w, 36, bar);
    p.rect(0, h - 60, w, 60, bar);
    auto headers = arr(o, "menu_headers");
    if (headers.empty())
      headers = J::array({{{"text", "◉"}, {"x", 0}, {"width", 46}},
                          {{"text", str(o, "app")}, {"x", 46}, {"width", 184}},
                          {{"text", "File"}, {"x", 230}, {"width", 50}},
                          {{"text", "Edit"}, {"x", 280}, {"width", 52}},
                          {{"text", "View"}, {"x", 332}, {"width", 58}},
                          {{"text", "Window"}, {"x", 390}, {"width", 80}},
                          {{"text", "Help"}, {"x", 470}, {"width", 58}}});
    int i = 0;
    auto clock = obj(o, "clock");
    int clockWidth=num(clock,"width",176), reserve=clockWidth+44;
    auto tray = obj(o, "tray");
    auto extras = arr(tray, "items");
    cairo_save(c);
    cairo_rectangle(c, 0, 0, std::max(0, w - reserve - int(extras.size()) * 28),
                    36);
    cairo_clip(c);
    for (auto &header : headers) {
      int x = num(header, "x"), width = num(header, "width");
      if (num(o, "menu", -1) == i)
        p.rect(x, 0, width, 35, p.sel);
      const auto icon = str(header, "icon");
      const int inset = icon.empty() ? 7 : 36;
      if (!icon.empty()) p.icon(x + 5, 6, 24, icon, p.fg);
      p.text(x + inset, 0, width - inset - 7, 35, str(header, "text"), p.fg);
      ++i;
    }
    cairo_restore(c);
    if (clock.empty()) {
      char date[32]; auto now = time(nullptr);
      strftime(date, sizeof(date), "%a %H:%M", localtime(&now));
      p.text(w - 168, 0, 152, 35, date, p.fg);
    } else {
      if (!obj(clock, "popup").empty()) p.rect(w - clockWidth, 0, clockWidth, 35, p.sel);
      auto second = str(clock, "secondary");
      p.text(w - clockWidth + 8, 0, clockWidth - 24, second.empty() ? 35 : 17, str(clock, "text"), p.fg);
      if (!second.empty()) p.text(w - clockWidth + 8, 17, clockWidth - 24, 18, second, p.fg);
    }
    if (yes(tray, "keyboard_menu") && !tray["menu"].is_null())
      p.rect(w - reserve, 0, 44, 35, p.sel);
    p.icon(w - reserve, 6, 24, "flag-" + str(o, "keyboard_flag", "generic"), p.fg, "", false);
    p.text(w - reserve + 25, 0, 19, 35, str(o, "keyboard"), p.fg);
    i = 0;
    for (auto &item : extras) {
      int x = w - reserve - (int(extras.size()) - i++) * 28 + 3;
      auto key = str(item, "key");
      bool selected =
          (!yes(tray, "keyboard_menu") && !tray["menu"].is_null() && num(tray, "menu", -1) == i - 1) ||
          (key == "dome.audio" && o["audio"].is_object() &&
           !o["audio"].empty()) ||
          (key == "dome.clipboard" &&
           str(obj(o, "history"), "kind") == "clipboard") ||
          (key == "dome.notifications" &&
           str(obj(o, "history"), "kind") == "notifications");
      if (selected)
        p.rect(x - 3, 0, 28, 35, p.sel);
      if (!p.image(x, 7, 22, 22, item, "pixmap", true))
        p.icon(x, 7, 22,
               key == "dome.audio" ? (str(item, "icon") == "muted" ||
                                              str(item, "icon") == "unavailable"
                                          ? "muted"
                                          : "speaker")
               : key == "dome.clipboard"     ? "clipboard"
               : key == "dome.notifications" ? "bell"
               : key == "dome.screenshot"    ? "camera-photo"
                                             : str(item, "icon"),
               p.fg, str(item, "icon_theme_path"), key.rfind("dome.", 0) != 0);
      if (yes(item, "attention"))
        p.rect(x + 7, 32, 8, 2, p.accent);
    }
    p.text(12, h - 60, 86, 60, "Launch", p.fg);
    p.text(w - 240, h - 60, 140, 60, "Desktops", p.fg);
    p.text(w - 90, h - 60, 86, 60, yes(o, "xr_active") ? "Desktop" : "XR",
           p.fg);
    i = 0;
    for (auto &task : arr(o, "tasks")) {
      int x = 110 + i++ * 130;
      if (x > w - 380)
        break;
      if (yes(task, "active"))
        p.rect(x - 5, h - 56, 126, 52, p.sel);
      p.icon(x + 4, h - 46, 28,
             str(task, "icon").empty() ? str(task, "name") : str(task, "icon"),
             yes(task, "minimized") ? p.muted : p.fg);
      p.text(x + 37, h - 51, 82, 36, str(task, "name"),
             yes(task, "minimized") ? p.muted : p.fg);
      auto count = arr(task, "windows").size();
      if (count)
        p.rect(x + 24, h - 7, 76, 3, yes(task, "active") ? p.accent : p.muted);
      if (count > 1)
        p.text(x + 96, h - 20, 22, 16, std::to_string(count), p.fg);
    }
    if (yes(o, "launcher")) {
      p.rect(20, 56, 600, 400, p.bg);
      p.text(36, 65, 560, 30, "Applications · " + str(o, "query"), p.fg);
      i = 0;
      for (auto &app : arr(o, "apps")) {
        int y = 106 + i * 36;
        if (i++ == num(o, "launch_index"))
          p.rect(28, y - 2, 584, 34, p.sel);
        p.icon(36, y, 24,
               str(app, "icon").empty() ? str(app, "name") : str(app, "icon"),
               p.fg);
        p.text(72, y, 280, 28, str(app, "name"), p.fg);
        auto categories = arr(app, "categories");
        if (!categories.empty() && categories[0].is_string())
          p.text(370, y, 230, 28, categories[0].get<std::string>(), p.muted);
      }
      p.text(36, 406, 560, 30, str(o, "launch_error"), p.accent);
    }
    if (yes(o, "manager")) {
      p.rect(w - 330, h - 390, 320, 320, p.bg);
      i = 0;
      for (auto &d : arr(o, "desktops")) {
        if (i == num(o, "shell_selection"))
          p.rect(w - 320, h - 384 + i * 32, 300, 32, p.sel);
        p.text(w - 310, h - 384 + i++ * 32, 280, 32,
               d.is_string() ? d.get<std::string>() : "Desktop", p.fg);
      }
      p.text(w - 310, h - 384 + i * 32, 280, 32, "+ New desktop", p.fg);
    }
    if (num(o, "menu", -1) >= 0 && !arr(o, "items").empty()) {
      int menu = num(o, "menu");
      p.menu(arr(o, "items"),
             menu < int(headers.size()) ? num(headers[menu], "x") : 0, 36, 380,
             num(o, "hover", -1));
    }
    for (auto &panel : arr(o, "appmenu_panels"))
      p.menu(arr(panel, "rows"), num(panel, "x"), num(panel, "y"),
             num(panel, "width"), num(panel, "selected", -1));
    if (tray.contains("menu") && tray["menu"].is_number_integer())
      p.menu(arr(tray, "rows"),
             std::clamp(yes(tray, "keyboard_menu") ? w - 284 : w - reserve - int(extras.size() - num(tray, "menu")) * 28,
                        4, std::max(4, w - 284)),
             36, 280, num(tray, "selected", -1));
    p.taskPopup(obj(o, "task_popup"));
    p.panelRows(obj(o, "xr_settings"), "Displays, XR & Keybindings", 28);
    p.panelRows(obj(o, "default_apps"),
                str(obj(o, "default_apps"), "title", "Default applications"),
                44, 64);
    p.panelRows(obj(o, "history"), str(obj(o, "history"), "title", "History"),
                64, 44);
    auto defaults = obj(o, "default_apps");
    if (!defaults.empty()) {
      int x = num(defaults, "x"), y = num(defaults, "y"),
          pw = num(defaults, "w"), ph = num(defaults, "h");
      p.button(x + 20, y + ph - 54, pw / 2 - 30, 36,
               yes(defaults, "chooser") ? "Back" : "Refresh");
      p.button(x + pw / 2 + 10, y + ph - 54, pw / 2 - 30, 36,
               yes(defaults, "chooser") ? "Use selected application" : "Close");
    }
    auto xr = obj(o, "xr_settings");
    if (!xr.empty()) {
      int x = num(xr, "x"), y = num(xr, "y"), pw = num(xr, "w"),
          ph = num(xr, "h");
      p.button(x + 20, y + ph - 66, 240, 36, "Anchor at headset");
      p.button(x + 280, y + ph - 66, 240, 36, "Restore defaults");
      p.button(x + pw - 140, y + ph - 66, 120, 36, "Close");
    }
    auto calendar = obj(obj(o, "clock"), "popup");
    if (!calendar.empty()) {
      int x = num(calendar, "x"), y = num(calendar, "y"), pw = num(calendar, "w"), ph = num(calendar, "h");
      cairo_save(c); cairo_rectangle(c, x, y, pw, ph); cairo_clip(c);
      p.rect(x, y, pw, ph, p.bg);
      p.button(x + 8, y + 4, 36, 34, "<");
      p.text(x + 48, y + 4, pw - 96, 34, str(calendar, "month"), p.fg);
      p.button(x + pw - 44, y + 4, 36, 34, ">");
      const char *week[] = {"Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"};
      double cell = (pw - 28.) / 7.;
      for (int col = 0; col < 7; ++col) p.text(x + 14 + col * cell, y + 44, cell, 26, week[col], p.muted);
      int index = 0;
      for (auto &day : arr(calendar, "days")) {
        int xx = x + 14 + index % 7 * cell, yy = y + 72 + index / 7 * 35; ++index;
        if (yes(day, "selected")) p.rect(xx, yy, cell, 34, p.sel);
        p.text(xx + 7, yy, cell - 14, 34, std::to_string(num(day, "day")), yes(day, "in_month") ? p.fg : p.muted);
        if (yes(day, "today")) p.rect(xx + 8, yy + 31, cell - 16, 2, p.accent);
      }
      p.button(x + 14, y + 290, pw - 28, 32, yes(calendar, "available") ? "Open date in Penguin Mail" : "Penguin Mail is not installed");
      p.button(x + 14, y + 330, pw - 28, 30, "Time display settings", yes(calendar, "settings_open"));
      if (yes(calendar, "settings_open")) {
        int row = 0;
        for (auto &option : arr(calendar, "options")) {
          if (row == num(calendar, "settings_row")) p.rect(x + 14, y + 370 + row * 32, pw - 28, 31, p.sel);
          if (option.is_array() && option.size() == 2) p.text(x + 22, y + 370 + row * 32, pw - 44, 31, std::string(option[1].get<bool>() ? "[x] " : "[ ] ") + option[0].get<std::string>(), p.fg);
          ++row;
        }
      }
      auto error = str(calendar, "error");
      if (!error.empty()) p.text(x + 14, y + ph - 18, pw - 28, 18, error, p.accent);
      cairo_restore(c);
    }
    auto audio = obj(o, "audio");
    if (!audio.empty()) {
      int x = num(audio, "x"), y = num(audio, "y"), pw = num(audio, "w"),
          ph = num(audio, "h");
      p.rect(x, y, pw, ph, p.bg);
      p.text(x + 16, y + 4, pw - 60, 32, "Audio", p.fg);
      p.text(x + pw - 36, y, 36, 36, "×", p.fg);
      for (int n = 0; n < 2; ++n) {
        int offset = n ? 136 : 64;
        auto level = obj(audio, n ? "input" : "output");
        double volume = level.value("volume", 0.);
        p.text(x + 16, y + offset - 28, pw - 32, 24,
               std::string(n ? "Microphone" : "Output") + " " +
                   std::to_string(int(volume * 100)) + "%",
               p.fg);
        p.button(x + 16, y + offset, 32, 30, "−");
        p.button(x + pw - 140, y + offset, 32, 30, "+");
        p.button(x + pw - 96, y + offset, 80, 30,
                 yes(level, "muted") ? "Unmute" : "Mute");
        p.rect(x + 60, y + offset + 11, pw - 216, 8, p.muted);
        p.rect(x + 60, y + offset + 11,
               std::clamp(int(volume * (pw - 216)), 0, std::max(0, pw - 216)),
               8, p.accent);
      }
      int first = num(audio, "first"), visible = num(audio, "visible");
      auto rows = arr(audio, "rows");
      for (int n = first; n < std::min(int(rows.size()), first + visible); ++n)
        p.text(x + 16, y + 214 + (n - first) * 32, pw - 32, 30,
               str(rows[n], "label"), p.fg);
    }
    if (yes(o, "appearance_open")) {
      bool keyboard =
          o.contains("keyboard_settings") && o["keyboard_settings"].is_object();
      auto fontChoice = obj(o, "appearance_font");
      bool scripts = yes(fontChoice, "has_scripts");
      int panelHeight = keyboard ? 400 : 560 + (scripts ? 48 : 0);
      int x = (w - 600) / 2, y = (h - panelHeight) / 2;
      p.rect(x, y, 600, panelHeight, p.bg);
      p.text(x + 24, y + 18, 552, 32,
             keyboard ? "Keyboard Layouts · desktop and XR"
                      : "Appearance & Input",
             p.fg);
      if (!keyboard) {
        auto a = obj(o, "appearance");
        std::vector<std::string> labels = {"Colour scheme",
                                           "Accent colour",
                                           "Chrome opacity",
                                           "Font family · BDF only",
                                           "Font face",
                                           "Font size",
                                           "Font scale",
                                           "Reset settings"};
        std::vector<std::string> values = {yes(a, "light") ? "Light" : "Dark",
                                           str(a, "accent"),
                                           std::to_string((num(a, "opacity", 255) * 100 + 127) / 255) + "%",
                                           str(a, "font").substr(0, str(a, "font").find(" / ")),
                                           str(a, "font").substr(str(a, "font").find(" / ") + 3),
                                           std::to_string(num(a, "font_size")) +
                                               " px · native",
                                           std::to_string(num(a, "font_scale", 1)) + "x",
                                           "Reset"};
        if (!fontChoice.empty()) {
          values[3] = str(fontChoice, "family"); values[4] = str(fontChoice, "face"); values[5] = str(fontChoice, "size");
        }
        if (scripts) { labels.insert(labels.begin() + 7, "Font script"); values.insert(values.begin() + 7, str(fontChoice, "script")); }
        for (int n = 0; n < int(labels.size()); ++n) {
          p.text(x + 24, y + 60 + n * 48, 270, 40, labels[n], p.fg);
          p.button(x + 300, y + 60 + n * 48, 276, 40, values[n],
                   num(o, "shell_selection") == n);
        }
        p.button(x + 24, y + panelHeight - 48, 260, 36, "Default applications...",
                 num(o, "shell_selection") == int(labels.size()));
        p.button(x + 316, y + panelHeight - 48, 260, 36, "Close · Esc",
                 num(o, "shell_selection") == int(labels.size()) + 1);
      } else {
        auto k = obj(o, "keyboard_settings");
        std::vector<std::string> keys = {"layout", "variant", "options"};
        for (int n = 0; n < 4; ++n) {
          p.text(x + 24, y + 60 + n * 48, 156, 40,
                 n < 3 ? keys[n] : "Switch shortcut", p.fg);
          p.button(x + 180, y + 60 + n * 48, 396, 40,
                   n < 3 ? str(k, keys[n].c_str())
                         : str(o, "keyboard_shortcut"),
                   num(o, "keyboard_field") == n);
        }
        p.text(x + 24, y + 252, 552, 36,
               yes(k, "ime") ? "US / Anthy / Czech / Hangul / Serbian"
                             : "Switch XKB groups",
               p.fg);
        p.button(x + 24, y + 344, 252, 40, "Apply · Enter");
        p.button(x + 324, y + 344, 252, 40, "Cancel · Esc");
      }
    }
    if (yes(o, "about")) {
      int x = (w - 440) / 2, y = (h - 200) / 2;
      p.rect(x, y, 440, 200, p.bg);
      p.text(x + 24, y + 24, 392, 152,
             "DomeD\nFLTK 1.4.5 experiment\nCairo/FreeType shell · Native "
             "OpenXR\nClick or press a key to close",
             p.fg);
    }
    if (o.contains("session_confirm") && o["session_confirm"].is_string()) {
      int x = (w - 440) / 2, y = (h - 200) / 2;
      p.rect(x, y, 440, 200, p.bg);
      p.text(x + 24, y + 20, 392, 60, str(o, "session_confirm") + "?", p.fg);
      p.button(x + 24, y + 140, 180, 40, "Cancel",
               num(o, "shell_selection") == 0);
      p.button(x + 230, y + 140, 186, 40, str(o, "session_confirm"),
               num(o, "shell_selection") == 1);
    }
  }
  for (auto &n : arr(o, "notifications")) {
    int x = num(n, "x"), y = num(n, "y"), nw = num(n, "w"), nh = num(n, "h");
    p.rect(x, y, nw, nh, p.bg);
    p.rect(x, y, 3, nh, p.accent);
    p.text(x + 12, y + 4, nw - 52, 22, str(n, "app"), p.muted);
    p.text(x + 12, y + 30, nw - 24, 30, str(n, "summary"), p.fg);
    auto image = obj(n, "image");
    if (!image.empty())
      p.image(x + 8, y + 32, nw - 16, nh - 36, image, "rgba");
    else {
      auto actions = arr(n, "actions");
      int count = 0;
      for (auto &action : actions)
        if (action.is_array() && action.size() == 2 && action[0] != "default")
          ++count;
      p.text(x + 12, y + 60, nw - 24, 36, str(n, "body"), p.fg);
      int index = 0;
      for (auto &action : actions)
        if (action.is_array() && action.size() == 2 && action[0] != "default") {
          int col = index % 2, row = index / 2;
          ++index;
          p.button(x + 8 + col * nw / 2, y + 102 + row * 32, nw / 2 - 16, 26,
                   action[1].get<std::string>());
        }
    }
    p.text(x + nw - 32, y, 32, 32, "×", p.fg);
  }
  cairo_destroy(c);
  cairo_surface_flush(surface);
  cairo_surface_destroy(surface);
  return 1;
} catch (...) {
  return 0;
}

#include "bdf_font_ffi.hpp"
