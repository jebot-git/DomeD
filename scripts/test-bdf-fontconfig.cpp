// SPDX-License-Identifier: MIT
#include "../native/bdf_font.hpp"
#include <iostream>

static void require(bool value, const char *message) {
  if (!value) throw std::runtime_error(message);
}

int main(int argc, char **argv) try {
  require(argc == 2, "Usage: test-bdf-fontconfig CACHE_HOME");
  setenv("XDG_CACHE_HOME", argv[1], 1);
  auto configuration = bdf_fonts::export_config();
  auto expected = std::filesystem::path(argv[1])/"de-xr/fontconfig/cache";
  require(std::filesystem::is_directory(expected), "Font cache directory created");
  auto config = FcConfigCreate();
  require(config && FcConfigParseAndLoad(config,
      reinterpret_cast<const FcChar8 *>(configuration.c_str()), FcTrue),
      "Exported Fontconfig configuration loads");
  auto directories = FcConfigGetCacheDirs(config);
  bool found = false;
  while (auto directory = FcStrListNext(directories))
    found |= expected == reinterpret_cast<const char *>(directory);
  FcStrListDone(directories);
  require(found, "Export includes its own writable cache directory");
  require(FcConfigBuildFonts(config), "Font scan succeeds");
  auto fonts = FcConfigGetFonts(config, FcSetSystem);
  require(fonts && fonts->nfont > 0, "Bitmap fonts remain available");
  for (int i = 0; i < fonts->nfont; ++i) {
    FcBool scalable = FcTrue;
    FcChar8 *format = nullptr;
    require(FcPatternGetBool(fonts->fonts[i], FC_SCALABLE, 0, &scalable) == FcResultMatch
        && scalable == FcFalse, "Outline fonts remain excluded");
    require(FcPatternGetString(fonts->fonts[i], FC_FONTFORMAT, 0, &format) == FcResultMatch
        && (!strcmp(reinterpret_cast<char *>(format), "BDF")
            || !strcmp(reinterpret_cast<char *>(format), "PCF")),
        "Only BDF and PCF fonts accepted");
  }
  bool cached = false;
  for (const auto &entry : std::filesystem::directory_iterator(expected))
    cached |= entry.path().filename().string().find("cache-") != std::string::npos;
  require(cached, "Font scan writes reusable cache files");
  std::cout << "PASS: " << fonts->nfont
            << " bitmap fonts, writable exported cache and reusable cache files\n";
  FcConfigDestroy(config);
} catch (const std::exception &error) {
  std::cerr << error.what() << '\n';
  return 1;
}
