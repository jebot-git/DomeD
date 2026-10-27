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
    if (ext != ".bdf") continue;
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
  auto overlay = dir / "overlay";
  std::filesystem::create_directories(overlay / "hicolor/22x22/apps");
  auto overlayIcon = overlay / "hicolor/22x22/apps/unique-overlay-icon.svg";
  std::filesystem::copy_file(icon, overlayIcon, std::filesystem::copy_options::overwrite_existing);
  require(theme_icons::resolve("unique-overlay-icon", overlay.string(), 22) == overlayIcon,
          "Icon overlay without its own index.theme was not found");
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
      auto test_surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 256, 160);
      auto test_cr = cairo_create(test_surface);
      Paint test(test_cr, {{"font", font.id}, {"font_size", font.sizes[0]}});
      char utf8[8] = {}; g_unichar_to_utf8(cp, utf8);
      test.text(0, 0, 256, 160, utf8, test.fg);
      cairo_surface_flush(test_surface);
      auto data = cairo_image_surface_get_data(test_surface);
      require(std::any_of(data, data + 256 * 160 * 4, [](unsigned char b) { return b != 0; }), "Missing UI glyph rendered empty");
      cairo_destroy(test_cr); cairo_surface_destroy(test_surface); checked = true;
    }
    y += 28;
  }
  require(checked, "No missing-glyph fallback case exercised");
  // Menu-bar identity must paint the application's real icon beside its name.
  auto iconPath = std::string(argv[2]) + ".header.svg";
  { std::ofstream icon(iconPath); icon << "<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><rect width='24' height='24' fill='#00ff00'/></svg>"; }
  J header = {{"kind", "shell"}, {"width", 640}, {"height", 160},
      {"menu_headers", J::array({{{"text", "Friendly Application"}, {"icon", std::filesystem::absolute(iconPath).string()}, {"x", 46}, {"width", 184}, {"enabled", true}}})}};
  std::vector<uint32_t> headerPixels(640 * 160);
  require(dome_qt_paint(header.dump().c_str(), reinterpret_cast<unsigned char *>(headerPixels.data()), headerPixels.size() * 4) == 1,
          "Application header painting failed");
  require((headerPixels[18 * 640 + 63] & 0xFFFFFF) == 0x00FF00,
          "Application icon missing from universal menu bar");
  std::filesystem::remove(iconPath);
  // Current desktop identity is independent of the switcher's keyboard cursor.
  for (bool light : {false, true}) {
    auto desktop = [light](int active, int selected, std::string name = "Writing", int count = 2) {
      J value = {{"kind", "shell"}, {"width", 640}, {"height", 480},
                 {"desktops", J::array({"Desktop 1", name, "Desktop 3"})},
                 {"active_workspace", active}, {"shell_selection", selected},
                 {"manager", true}, {"appearance", {{"light", light}}}};
      value["workspace_switcher"] = {{"layout", {{"x",70},{"y",206},{"w",560},{"h",204},
          {"row_y",238},{"row_height",36},{"count",3},{"can_add",true}}},
          {"rows",J::array({{{"name","Desktop 1"},{"windows",0}},{{"name",name},{"windows",count}},
                            {{"name","Desktop 3"},{"windows",1}}})}};
      std::vector<uint32_t> pixels(640 * 480);
      require(dome_qt_paint(value.dump().c_str(), (unsigned char *)pixels.data(), pixels.size() * 4) == 1,
              "Desktop switcher painting failed");
      return pixels;
    };
    auto first = desktop(0, 0), second = desktop(1, 1), cursor = desktop(1, 2);
    auto renamed = desktop(1, 2, "作業 · Žluťoučký desktop with a long name");
    auto regionDiffers = [](const auto &a, const auto &b, int x, int y, int w, int h) {
      for (int row = y; row < y + h; ++row) for (int col = x; col < x + w; ++col)
        if (a[row * 640 + col] != b[row * 640 + col]) return true;
      return false;
    };
    require(regionDiffers(first, second, 400, 422, 140, 28), "Bar desktop number did not update");
    require(regionDiffers(second, renamed, 400, 450, 140, 28), "Bar omitted the current desktop name");
    require(!regionDiffers(second, cursor, 390, 420, 160, 60), "Keyboard cursor changed current desktop identity");
    const int marker = 83;
    require(first[256 * 640 + marker] == second[292 * 640 + marker], "Active desktop marker did not follow the workspace");
    require(second[292 * 640 + marker] == cursor[292 * 640 + marker], "Keyboard cursor moved the active desktop marker");
    require(cursor[292 * 640 + marker] != cursor[328 * 640 + marker], "Keyboard target was marked as the active desktop");
    require(!regionDiffers(cursor, renamed, 540, 420, 100, 60), "Long desktop name escaped into the XR button");
    auto empty = desktop(1,2,"Writing",0);
    require(regionDiffers(cursor,empty,440,274,96,36), "Window occupancy is not visible beside the desktop");
    auto image = cairo_image_surface_create_for_data((unsigned char *)cursor.data(), CAIRO_FORMAT_ARGB32, 640, 480, 640 * 4);
    cairo_surface_write_to_png(image, (std::string(argv[2]) + (light ? ".desktop-light.png" : ".desktop-dark.png")).c_str());
    cairo_surface_destroy(image);
  }
  // Button pixels must be identical across fonts and native sizes.
  std::vector<unsigned char> reference;
  for (auto &font : fonts) {
    for (int size : font.sizes) {
      J value = {{"kind", "frame"}, {"width", 240}, {"height", 80}, {"appearance", {{"font", font.id}, {"font_size", 16}}}};
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
  }
  auto frame = [](bool light, bool active, bool zoomed, int pressed, int opacity = 255) {
    J value = {{"kind", "frame"}, {"width", 240}, {"height", 80},
               {"active", active}, {"zoomed", zoomed}, {"pressed", pressed},
               {"appearance", {{"light", light}, {"opacity", opacity}}}};
    std::vector<uint32_t> pixels(240 * 80);
    require(dome_qt_paint(value.dump().c_str(), (unsigned char *)pixels.data(), pixels.size() * 4) == 1,
            "Workbench frame painting failed");
    return pixels;
  };
  // Check what the gadgets communicate, in both palettes and focus states.
  for (bool light : {false, true}) for (bool active : {false, true}) {
    auto normal = frame(light, active, false, 0), zoomed = frame(light, active, true, 0);
    auto ink = normal[14 * 240 + 14], fill = normal[5 * 240 + 14];
    require(ink != fill, "Close square has no contrast");
    for (int y = 10; y <= 18; ++y) for (int x = 10; x <= 18; ++x)
      require(normal[y * 240 + x] == ink, "Close must be a solid square, not a minimize line");
    require(normal[13 * 240 + 191] == fill && zoomed[13 * 240 + 191] == ink,
            "Zoom must show its two preset states");
    require(normal[9 * 240 + 218] == ink && normal[21 * 240 + 232] == ink && normal[16 * 240 + 225] == fill,
            "Depth must show overlapping window planes, not a close cross");
    auto down = frame(light, active, false, 1);
    require(down[10 * 240 + 10] != ink && down[19 * 240 + 19] == ink,
            "Pressed close must move one pixel down and right");
    require(normal[1 * 240 + 14] == down[28 * 240 + 14] && normal[28 * 240 + 14] == down[1 * 240 + 14],
            "Pressed gadget must reverse its bevel");
    auto translucent = frame(light, active, false, 6, 128);
    require(std::all_of(translucent.begin(), translucent.begin() + 240 * 30, [](uint32_t pixel) { return pixel >> 24 == 128; }),
            "Gadgets and caption must share the configured chrome opacity");
  }
  require(dome_qt_hit(240, 80, 14, 14) == 1 && dome_qt_hit(240, 80, 194, 14) == 3 && dome_qt_hit(240, 80, 224, 14) == 6,
          "Workbench icons must match close/zoom/depth release actions");
  for (auto point : {std::pair<int,int>{2,68}, {14,78}})
    require(dome_qt_hit(240,80,point.first,point.second)==7,"Bottom-left rim must resize diagonally over both legs");
  for (auto point : {std::pair<int,int>{238,68}, {226,78}})
    require(dome_qt_hit(240,80,point.first,point.second)==8,"Bottom-right rim must resize diagonally over both legs");
  require(dome_qt_hit(240,80,-2,68)==7 && dome_qt_hit(240,80,14,82)==7 &&
          dome_qt_hit(240,80,242,68)==8 && dome_qt_hit(240,80,226,82)==8,
          "Bottom corners must provide an outward four-pixel grab margin");
  require(dome_qt_hit(240,80,-5,68)==0 && dome_qt_hit(240,80,14,84)==0 && dome_qt_hit(240,80,-2,40)==0,
          "Corner grab margins must stay bounded and leave straight-edge padding transparent");
  require(dome_qt_hit(240,80,14,68)==0 && dome_qt_hit(240,80,226,68)==0,
          "Corner grips must never consume application content");
  require(dome_qt_hit(240,80,120,78)==5 && dome_qt_hit(240,80,2,55)==5,
          "Straight rim segments must retain single-axis resizing");
  for (bool light : {false,true}) for(bool active : {false,true}) {
    auto pixels=frame(light,active,false,0,128);
    require((pixels[68*240+2]>>24) > (pixels[78*240+120]>>24),
            "Corner grips must be more visible than the straight rim in both palettes and focus states");
    require((pixels[78*240+2]>>24)<=128,"Corner overlap must respect configured chrome opacity");
    require(pixels[68*240+14]==0 && pixels[68*240+226]==0,"Painting must preserve transparent client content");
  }
  {
    auto pixels=frame(false,true,false,0);
    auto image=cairo_image_surface_create_for_data((unsigned char*)pixels.data(),CAIRO_FORMAT_ARGB32,240,80,240*4);
    cairo_surface_write_to_png(image,(std::string(argv[2])+".resize-rims.png").c_str());cairo_surface_destroy(image);
  }
  // A separate contact sheet makes light/dark, inactive and pressed states reviewable.
  auto preview = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 480, 240);
  auto preview_cr = cairo_create(preview);
  int row = 0;
  for (bool active : {true, false}) for (int pressed : {0, 1, 3, 6}) {
    for (bool light : {false, true}) {
      auto pixels = frame(light, active, pressed == 3, pressed);
      auto image = cairo_image_surface_create_for_data((unsigned char *)pixels.data(), CAIRO_FORMAT_ARGB32, 240, 80, 240 * 4);
      cairo_save(preview_cr); cairo_rectangle(preview_cr, light ? 240 : 0, row * 30, 240, 30); cairo_clip(preview_cr);
      cairo_set_source_surface(preview_cr, image, light ? 240 : 0, row * 30); cairo_paint(preview_cr);
      cairo_restore(preview_cr); cairo_surface_destroy(image);
    }
    ++row;
  }
  cairo_surface_write_to_png(preview, (std::string(argv[2]) + ".gadgets.png").c_str());
  cairo_destroy(preview_cr); cairo_surface_destroy(preview);
  cairo_surface_write_to_png(surface, argv[2]);
  cairo_destroy(cr); cairo_surface_destroy(surface);
  std::filesystem::remove_all(dir);
  std::cout << "PASS: theme icons, bitmap fallback, current desktop indicators, font-independent Workbench close/zoom/depth gadgets\n";
} catch (const std::exception &e) { std::cerr << e.what() << '\n'; return 1; }
