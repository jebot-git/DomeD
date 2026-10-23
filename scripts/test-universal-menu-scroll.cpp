// SPDX-License-Identifier: MIT
// Headless regression: long exported menus must stop at their viewport.
#include "../native/fltk_chrome.cpp"
#include <fstream>
#include <iostream>
#include <stdexcept>
int main(int argc, char **argv) try {
  if (argc != 3) throw std::runtime_error("Usage: test-universal-menu-scroll FONT.bdf OUTPUT.png");
  std::ifstream font(argv[1], std::ios::binary);
  std::vector<unsigned char> bytes((std::istreambuf_iterator<char>(font)), {});
  dome_qt_queue_bitmap_font(bytes.data(), bytes.size());
  dome_qt_font_catalog(nullptr, 0);
  auto surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 640, 480);
  auto cr = cairo_create(surface);
  cairo_set_source_rgb(cr, 1, 0, 1); cairo_paint(cr);
  Paint p(cr, J::object());
  J rows = J::array();
  for (int i = 0; i < 80; ++i) rows.push_back({{"id", 1000 + i * 7}, {"text", "Action " + std::to_string(i)}, {"enabled", true}});
  p.menu(rows, 20, 36, 380, 79, {{"height", 244}, {"offset", 2044}});
  cairo_surface_flush(surface);
  auto data = reinterpret_cast<uint32_t *>(cairo_image_surface_get_data(surface));
  for (int y = 288; y < 480; ++y)
    for (int x = 0; x < 640; ++x)
      if ((data[y * 640 + x] & 0xffffff) != 0xff00ff)
        throw std::runtime_error("Menu rows escaped the bounded panel");
  auto pixel = data[246 * 640 + 390] & 0xffffff;
  p.rect(500, 0, 10, 10, p.sel); cairo_surface_flush(surface);
  if (pixel != (data[5 * 640 + 505] & 0xffffff))
    throw std::runtime_error("Last selected remote action is not revealed");
  cairo_surface_write_to_png(surface, argv[2]);
  cairo_destroy(cr); cairo_surface_destroy(surface);
  std::cout << "Long menu clipping, selection visibility and clip restoration passed\n";
} catch (const std::exception &error) { std::cerr << error.what() << '\n'; return 1; }
