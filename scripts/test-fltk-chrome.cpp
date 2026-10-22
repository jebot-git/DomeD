// Headless regression checks for theme icons, bitmap fallback and decorations.
#include "../native/fltk_chrome.cpp"
#include <fstream>
#include <iostream>
#include <stdexcept>
void require(bool ok, const char *message) { if (!ok) throw std::runtime_error(message); }
int main(int argc, char **argv) try {
  require(argc == 3, "Usage: test-fltk-chrome FONT_DIRECTORY OUTPUT.png");
  for (const auto &entry : std::filesystem::recursive_directory_iterator(argv[1])) {
    auto ext = entry.path().extension();
    if (ext != ".pcf" && ext != ".bdf" && ext != ".otb" && ext != ".ttf") continue;
    if (entry.path().filename() == "upstream.bdf") continue;
    std::ifstream in(entry.path(), std::ios::binary);
    std::vector<unsigned char> bytes((std::istreambuf_iterator<char>(in)), {});
    dome_qt_queue_bitmap_font(bytes.data(), bytes.size());
  }
  auto catalog = J::parse(dome_qt_font_catalog(nullptr, 0));
  require(!catalog.empty(), "Bitmap font catalog empty");
  fl_register_images();
  auto surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 640, 480);
  auto cr = cairo_create(surface);
  cairo_set_source_rgb(cr, .12, .145, .176); cairo_paint(cr);
  Paint p(cr, J::object());
  for (int i = 0; i < 9; ++i) {
    const char *flags[] = {"us", "jp", "cz", "kr", "rs", "de", "gb", "fr", "generic"};
    for (bool light : {false, true}) {
      p.rect(40 + i * 32, light ? 32 : 0, 32, 32, light ? Color{.9,.92,.94} : p.bg);
      p.icon(40 + i * 32, light ? 36 : 4, 24, std::string("flag-") + flags[i],
             light ? Color{.1,.15,.2} : p.fg, "", false);
    }
  }
  // A private application IconThemePath, using an inherited theme.
  auto dir = std::filesystem::temp_directory_path() / ("dome-icons-test-" + std::to_string(getpid()));
  std::filesystem::create_directories(dir / "Parent/22x22/apps");
  std::filesystem::create_directories(dir / "hicolor");
  std::ofstream(dir / "hicolor/index.theme") << "[Icon Theme]\nInherits=Parent\nDirectories=\n";
  std::ofstream(dir / "Parent/index.theme") << "[Icon Theme]\nDirectories=22x22/apps\n[22x22/apps]\nSize=22\nType=Fixed\n";
  auto icon = dir / "Parent/22x22/apps/unique-test-icon.svg";
  std::ofstream(icon) << "<svg xmlns='http://www.w3.org/2000/svg' width='22' height='22'><rect width='22' height='22' fill='#ec172a'/></svg>";
  require(theme_icons::resolve("unique-test-icon", dir.string(), 22) == icon, "Inherited theme icon was not found");
  require(theme_icons::resolve(icon.string(), "", 22) == icon, "Absolute icon was not found");
  p.icon(0, 0, 22, "unique-test-icon", p.fg, dir.string());
  cairo_surface_flush(surface);
  auto pixels = (uint32_t *)cairo_image_surface_get_data(surface);
  require((pixels[16 * 640 + 84] & 0xffffff) == 0xbe6570, "Japanese flag missing its national colour in dark appearance");
  require((pixels[48 * 640 + 84] & 0xffffff) == 0xbe6570, "Japanese flag colour changed in light appearance");
  require((pixels[10 * 640 + 10] & 0xffffff) == 0xec172a, "Theme icon replaced by generic icon or recoloured");
  // Exercise a font which lacks a UI symbol covered by another bundled font.
  bool checked = false; int y = 40;
  for (auto &font : fonts) {

    Paint sample(cr, {{"font", font.id}, {"font_size", font.sizes[0]}});
    if (y <= 440) sample.text(0, y, 640, 28, font.id + "  ◉ ← → ⌥ ⌘ ▲ ▣ é Ж", sample.fg);
    for (auto cp : {0x25c9, 0x2325, 0x2318, 0x2190}) {
      if (FT_Get_Char_Index(sample.face, cp)) continue;
      auto fallback = std::find_if(fonts.begin(), fonts.end(), [cp](const Font &f) { return FT_Get_Char_Index(f.face, cp); });
      if (fallback == fonts.end()) continue;
      auto test_surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 40, 32);
      auto test_cr = cairo_create(test_surface);
      Paint test(test_cr, {{"font", font.id}, {"font_size", font.sizes[0]}});
      char utf8[8] = {}; g_unichar_to_utf8(cp, utf8);
      test.text(0, 0, 40, 32, utf8, test.fg);
      cairo_surface_flush(test_surface);
      auto data = cairo_image_surface_get_data(test_surface);
      require(std::any_of(data, data + 40 * 32 * 4, [](unsigned char b) { return b != 0; }), "Missing UI glyph rendered empty");
      cairo_destroy(test_cr); cairo_surface_destroy(test_surface); checked = true;
    }
    y += 28;
  }
  require(checked, "No missing-glyph fallback case exercised");
  // Button pixels must be identical across fonts and native sizes.
  std::vector<unsigned char> reference;
  for (auto &font : fonts) {
    J value = {{"kind", "frame"}, {"width", 240}, {"height", 80}, {"appearance", {{"font", font.id}, {"font_size", font.sizes[0]}}}};
    std::vector<unsigned char> bytes(240 * 80 * 4);
    require(dome_qt_paint(value.dump().c_str(), bytes.data(), bytes.size()) == 1, "Decoration painting failed");
    std::vector<unsigned char> buttons;
    for (int row = 0; row < 30; ++row) {
      buttons.insert(buttons.end(), bytes.begin() + row * 240 * 4, bytes.begin() + (row * 240 + 30) * 4);
      buttons.insert(buttons.end(), bytes.begin() + (row * 240 + 180) * 4, bytes.begin() + (row + 1) * 240 * 4);
    }
    if (reference.empty()) reference = buttons;
    require(reference == buttons, "Decoration symbols changed with font");
  }
  cairo_surface_write_to_png(surface, argv[2]);
  cairo_destroy(cr); cairo_surface_destroy(surface);
  std::filesystem::remove_all(dir);
  std::cout << "PASS: inherited/absolute theme icons, original icon colours, bitmap fallback, font-independent decorations\n";
} catch (const std::exception &e) { std::cerr << e.what() << '\n'; return 1; }
