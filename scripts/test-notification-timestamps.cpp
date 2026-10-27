// SPDX-License-Identifier: MIT
// Headless regression for local timestamps and shared desktop/XR notification rows.
#include "../native/fltk_chrome.cpp"
#include <fstream>
#include <iostream>
#include <stdexcept>

static void require(bool ok, const char *message) {
  if (!ok) throw std::runtime_error(message);
}
static std::vector<uint32_t> render(const J &model) {
  std::vector<uint32_t> pixels(num(model, "width") * num(model, "height"));
  require(dome_qt_paint(model.dump().c_str(), reinterpret_cast<unsigned char *>(pixels.data()), pixels.size() * 4),
          "Notification painting failed");
  return pixels;
}
static void save(const std::vector<uint32_t> &pixels, int width, int height, const std::filesystem::path &path) {
  auto surface = cairo_image_surface_create_for_data(reinterpret_cast<unsigned char *>(const_cast<uint32_t *>(pixels.data())),
      CAIRO_FORMAT_ARGB32, width, height, width * 4);
  require(cairo_surface_write_to_png(surface, path.c_str()) == CAIRO_STATUS_SUCCESS, "Cannot save notification capture");
  cairo_surface_destroy(surface);
}
int main(int argc, char **argv) try {
  require(argc == 3, "Usage: test-notification-timestamps FONT.bdf OUTPUT_DIRECTORY");
  setenv("TZ", "UTC0", 1); tzset();
  const J stamp{{"timestamp", 1700000000}};
  require(notificationTimestamp(stamp, false) == "22:13:20", "Popup must show receipt time with seconds");
  require(notificationTimestamp(stamp, true) == "2023-11-14 22:13:20", "History must show the receipt date and time");
  setenv("TZ", "UTC-2", 1); tzset();
  require(notificationTimestamp(stamp, true) == "2023-11-15 00:13:20", "Receipt time must use the local timezone, including date changes");
  setenv("TZ", "UTC0", 1); tzset();
  for (const auto &value : {J::object(), J{{"timestamp", -1}}, J{{"timestamp", "invalid"}},
      J{{"timestamp", 1.5}}, J{{"timestamp", std::numeric_limits<uint64_t>::max()}}})
    require(notificationTimestamp(value, true).empty(), "Invalid or missing timestamps must not invent a receipt time");

  std::ifstream font(argv[1], std::ios::binary);
  require(bool(font), "Cannot open test font");
  std::vector<unsigned char> bytes((std::istreambuf_iterator<char>(font)), {});
  dome_qt_queue_bitmap_font(bytes.data(), bytes.size());
  dome_qt_font_catalog(nullptr, 0);
  std::filesystem::create_directories(argv[2]);
  for (bool history : {false, true}) for (bool light : {false, true}) for (int scale : {1, 2}) {
    J row{{"timestamp", 1700000000}, {"app", "A very long application origin that must stop before the timestamp"},
        {"detail", "A very long application origin that must stop before the timestamp"}, {"text", "Summary\nBody"},
        {"summary", "Summary"}, {"body", "Body"}, {"actions", J::array()}, {"actionable", true},
        {"x", 628}, {"y", 48}, {"w", 360}, {"h", 100}};
    const int lineHeight = std::max(20, 16 * scale), headerHeight = std::max(32, 16 * scale + 10);
    row["header_height"] = headerHeight;
    row["summary_height"] = std::max(30, 16 * scale + 2);
    row["body_height"] = std::max(38, 16 * scale + 2);
    row["action_height"] = std::max(32, 16 * scale + 8);
    row["h"] = headerHeight + row["summary_height"].get<int>() + row["body_height"].get<int>();
    J model{{"kind", history ? "shell" : "notifications"}, {"width", 1000}, {"height", 600},
        {"appearance", {{"light", light}, {"font_size", 16}, {"font_scale", scale}}}};
    if (history)
      model["history"] = {{"kind", "notifications"}, {"title", "Notification history"}, {"x", 20}, {"y", 42},
          {"w", 760}, {"h", 300}, {"row_height", lineHeight * 3 + 4}, {"line_height", lineHeight}, {"first", 0}, {"visible", 3}, {"selected", 0}, {"rows", J::array({row})}};
    else model["notifications"] = J::array({row});
    auto &notice = history ? model["history"]["rows"][0] : model["notifications"][0];
    const auto after = render(model);
    notice.erase("timestamp");
    const auto before = render(model);
    notice["timestamp"] = 1700000000;
    notice["app"] = ""; notice["detail"] = ""; notice["actionable"] = false;
    const auto onlyTime = render(model);
    const int x = history ? 38 : 640, y = history ? 88 + lineHeight * 2 : 52;
    const int width = history ? 690 : 308, height = history ? lineHeight : headerHeight - 10;
    auto surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 1, 1);
    auto cr = cairo_create(surface);
    Paint p(cr, model["appearance"]);
    const int timeWidth = p.timestampWidth(notificationTimestamp(stamp, history));
    cairo_destroy(cr); cairo_surface_destroy(surface);
    require(timeWidth > 0 && timeWidth < width, "Timestamp must fit at both supported font scales");
    bool changed = false, timeInk = false;
    for (int py = 0; py < 600; ++py) for (int px = 0; px < 1000; ++px) {
      const auto index = py * 1000 + px;
      const bool inRow = px >= x && px < x + width && py >= y && py < y + height;
      if (!inRow) require(before[index] == after[index], "Timestamp changed notification content, controls or row geometry");
      else changed |= before[index] != after[index];
      if (inRow && px >= x + width - timeWidth) {
        require(after[index] == onlyTime[index], "Origin text overlaps the timestamp at the right edge");
        timeInk |= after[index] != after[y * 1000 + x + width - 1];
      }
    }
    require(changed && timeInk, "Timestamp must be visibly rendered on the origin row");
    save(after, 1000, 600, std::filesystem::path(argv[2]) /
        (std::string(history ? "history" : "popup") + (light ? "-light" : "-dark") + "-" + std::to_string(scale) + "x.png"));
    if (history) {
      model["history"]["kind"] = "clipboard";
      const auto clipboard = render(model);
      notice.erase("timestamp");
      require(clipboard == render(model), "Notification timestamp changes must leave clipboard rows unchanged");
    }
  }
  std::cout << "PASS: local receipt date/time, right alignment, origin clipping, popup/history rows, both palettes and font scales\n";
} catch (const std::exception &error) { std::cerr << error.what() << '\n'; return 1; }
