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
};
std::vector<Font> fonts;
std::vector<std::vector<unsigned char>> bundled;
void add(FT_Face face) {
  if (FT_IS_SCALABLE(face) || !face->num_fixed_sizes ||
      FT_Select_Charmap(face, FT_ENCODING_UNICODE) ||
      !FT_Get_Char_Index(face, 'A') ||
      !(FT_Get_Char_Index(face, 0xe9) || FT_Get_Char_Index(face, 0x3042) ||
        FT_Get_Char_Index(face, 0xac00))) {
    FT_Done_Face(face);
    return;
  }
  std::vector<int> sizes;
  for (int i = 0; i < face->num_fixed_sizes; ++i) {
    int n = std::lround(face->available_sizes[i].y_ppem / 64.);
    if (n >= 12 && n <= 24)
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
       std::string(face->family_name ? face->family_name : "Bitmap") + " / " +
           (face->style_name ? face->style_name : "Regular"),
       sizes});
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
  Color bg, fg, muted, sel, accent;
  double opacity = 1;
  Paint(cairo_t *cr, const J &a) : c(cr) {
    bool light = yes(a, "light");
    bg = light ? Color{.90, .91, .94} : Color{.12, .145, .176};
    fg = light ? Color{.12, .145, .18} : Color{.906, .922, .945};
    muted = light ? Color{.41, .45, .52} : Color{.57, .60, .65};
    sel = light ? Color{.78, .84, .91} : Color{.247, .302, .376};
    accent = hex(str(a, "accent", "#6babff"), {.42, .67, 1});
    opacity = std::clamp(num(a, "opacity", 248), 0, 255) / 255.;
    auto id = str(a, "font", "Unifont / Sans-Serif");
    for (auto &f : fonts)
      if (f.id == id) {
        face = f.face;
        break;
      }
    if (!face && !fonts.empty())
      face = fonts[0].face;
    size = num(a, "font_size", 16);
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
      cairo_select_font_face(c, "sans", CAIRO_FONT_SLANT_NORMAL,
                             CAIRO_FONT_WEIGHT_NORMAL);
      cairo_set_font_size(c, size);
      cairo_move_to(c, cursor, baseline);
      cairo_show_text(c, s.c_str());
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
          glyph_face = fallback.face;
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
    if (label == "■" || label == "□" || label == "▣") {
      int cx = x + w / 2, cy = y + h / 2;
      color(fg); cairo_set_line_width(c, 1.5);
      if (label == "■") { cairo_move_to(c, cx - 5, cy + 4); cairo_line_to(c, cx + 5, cy + 4); }
      else if (label == "□") cairo_rectangle(c, cx - 5, cy - 5, 10, 10);
      else { cairo_move_to(c, cx - 5, cy - 5); cairo_line_to(c, cx + 5, cy + 5);
             cairo_move_to(c, cx + 5, cy - 5); cairo_line_to(c, cx - 5, cy + 5); }
      cairo_stroke(c);
    } else text(x + 8, y, w - 16, h, label, fg);
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
  if (!result.empty())
    return result.c_str();
  if (FT_Init_FreeType(&library)) {
    result = "[]";
    return result.c_str();
  }
  dome_qt_queue_bitmap_font(data, length);
  for (auto &bytes : bundled) {
    FT_Face f;
    if (!FT_New_Memory_Face(library, bytes.data(), bytes.size(), 0, &f))
      add(f);
  }
  std::set<std::string> paths;
  auto query = FcPatternCreate();
  FcPatternAddBool(query, FC_SCALABLE, FcFalse);
  auto objects = FcObjectSetBuild(FC_FILE, nullptr);
  auto matches = FcFontList(nullptr, query, objects);
  if (matches) {
    for (int i = 0; i < matches->nfont; ++i) {
      FcChar8 *file;
      if (FcPatternGetString(matches->fonts[i], FC_FILE, 0, &file) ==
          FcResultMatch)
        paths.emplace((char *)file);
    }
    FcFontSetDestroy(matches);
  }
  FcObjectSetDestroy(objects);
  FcPatternDestroy(query);
  for (auto &path : paths) {
    FT_Face f;
    if (!FT_New_Face(library, path.c_str(), 0, &f))
      add(f);
  }
  J catalog = J::array();
  std::set<std::string> seen;
  for (auto &f : fonts)
    if (seen.insert(f.id).second)
      catalog.push_back(
          {{"id", f.id},
           {"family", f.face->family_name},
           {"style", f.face->style_name ? f.face->style_name : "Regular"},
           {"sizes", f.sizes}});
  result = catalog.dump();
  return result.c_str();
}
extern "C" int dome_qt_hit(int w, int h, int x, int y) {
  if (x < 0 || y < 0 || x >= w || y >= h)
    return 0;
  if (y < 30) {
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
    auto caption = yes(o, "active") ? p.sel : p.bg;
    caption.a = p.opacity;
    p.rect(0, 0, w, 30, caption);
    auto border = p.accent;
    border.a = .4;
    p.rect(0, 30, 4, h - 30, border);
    p.rect(w - 4, 30, 4, h - 30, border);
    p.rect(0, h - 4, w, 4, border);
    p.text(38, 0, w - 106, 30, str(o, "title"), p.fg);
    p.button(1, 1, 28, 28, "■", num(o, "pressed") == 1);
    p.button(w - 59, 1, 28, 28, "□", num(o, "pressed") == 3);
    p.button(w - 29, 1, 28, 28, "▣", num(o, "pressed") == 6);
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
    auto tray = obj(o, "tray");
    auto extras = arr(tray, "items");
    cairo_save(c);
    cairo_rectangle(c, 0, 0, std::max(0, w - 220 - int(extras.size()) * 28),
                    36);
    cairo_clip(c);
    for (auto &header : headers) {
      int x = num(header, "x"), width = num(header, "width");
      if (num(o, "menu", -1) == i)
        p.rect(x, 0, width, 35, p.sel);
      p.text(x + 7, 0, width - 14, 35, str(header, "text"), p.fg);
      ++i;
    }
    cairo_restore(c);
    char date[32];
    auto now = time(nullptr);
    strftime(date, sizeof(date), "%a %H:%M", localtime(&now));
    p.text(w - 168, 0, 152, 35, date, p.fg);
    if (yes(tray, "keyboard_menu") && !tray["menu"].is_null())
      p.rect(w - 220, 0, 44, 35, p.sel);
    p.icon(w - 220, 6, 24, "flag-" + str(o, "keyboard_flag", "generic"), p.fg, "", false);
    p.text(w - 195, 0, 19, 35, str(o, "keyboard"), p.fg);
    i = 0;
    for (auto &item : extras) {
      int x = w - 220 - (int(extras.size()) - i++) * 28 + 3;
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
             std::clamp(yes(tray, "keyboard_menu") ? w - 284 : w - 220 - int(extras.size() - num(tray, "menu")) * 28,
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
      int x = (w - 600) / 2, y = (h - (keyboard ? 400 : 500)) / 2;
      p.rect(x, y, 600, keyboard ? 400 : 500, p.bg);
      p.text(x + 24, y + 18, 552, 32,
             keyboard ? "Keyboard Layouts · desktop and XR"
                      : "Appearance & Input",
             p.fg);
      if (!keyboard) {
        auto a = obj(o, "appearance");
        std::vector<std::string> labels = {"Colour scheme",
                                           "Accent colour",
                                           "Desktop background",
                                           "Chrome opacity",
                                           "Global font · bitmap only",
                                           "Font size",
                                           "Reset settings"};
        std::vector<std::string> values = {yes(a, "light") ? "Light" : "Dark",
                                           str(a, "accent"),
                                           "Change",
                                           std::to_string(num(a, "opacity")),
                                           str(a, "font"),
                                           std::to_string(num(a, "font_size")) +
                                               " px · native",
                                           "Reset"};
        for (int n = 0; n < 7; ++n) {
          p.text(x + 24, y + 60 + n * 48, 270, 40, labels[n], p.fg);
          p.button(x + 300, y + 60 + n * 48, 276, 40, values[n],
                   num(o, "shell_selection") == n);
        }
        p.button(x + 24, y + 452, 260, 36, "Default applications...",
                 num(o, "shell_selection") == 7);
        p.button(x + 316, y + 452, 260, 36, "Close · Esc",
                 num(o, "shell_selection") == 8);
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
