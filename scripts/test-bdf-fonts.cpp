// SPDX-License-Identifier: MIT
#include "../native/bdf_font.hpp"
#include <ft2build.h>
#include FT_FREETYPE_H
#include FT_FONT_FORMATS_H
#include <iostream>
static void require(bool value, const char *message) { if (!value) throw std::runtime_error(message); }
int main(int argc, char **argv) try {
  require(argc == 2, "Usage: test-bdf-fonts FONT_DIRECTORY");
  setenv("DE_XR_BDF_DIRECTORY", argv[1], 1);
  auto discovered = bdf_fonts::discover(true);
  FT_Library library; require(!FT_Init_FreeType(&library), "FreeType init");
  int count = 0;
  for (const auto &entry : std::filesystem::recursive_directory_iterator(argv[1])) {
    if (entry.path().extension() != ".bdf" || entry.path().filename() == "upstream.bdf") continue;
    std::cerr << entry.path() << "\n";
    bdf_fonts::Face base; require(bdf_fonts::inspect(entry.path().string(), base), "Native BDF metadata");
    require(std::any_of(discovered.begin(), discovered.end(), [&](const auto &f) { return f.file == base.file; }), "BDF discovered through Fontconfig directories");
    auto enlarged = bdf_fonts::scaled(base, 2);
    require(enlarged.pixels == base.pixels * 2, "Doubled pixel size");
    require(enlarged.family == "Dome 2x " + base.family, "Distinct 2x family");
    FT_Face one, two;
    require(!FT_New_Face(library, base.file.c_str(), 0, &one) && !FT_New_Face(library, enlarged.file.c_str(), 0, &two), "Both native BDFs load");
    require(!strcmp(FT_Get_Font_Format(two), "BDF"), "Scaling must retain BDF format");
    FT_Select_Charmap(one, FT_ENCODING_UNICODE); FT_Select_Charmap(two, FT_ENCODING_UNICODE);
    FT_Select_Size(one, 0); FT_Select_Size(two, 0);
    for (unsigned cp : {77u, 0xe9u, 0x65e5u, 0xd55cu, 0x0627u, 0x0e01u}) {
      if (!FT_Get_Char_Index(one, cp)) continue;
      require(!FT_Load_Char(one, cp, FT_LOAD_DEFAULT) && !FT_Load_Char(two, cp, FT_LOAD_DEFAULT), "Glyph load");
      auto &a = one->glyph->bitmap; auto &b = two->glyph->bitmap;
      require(b.width == a.width * 2 && b.rows == a.rows * 2 && two->glyph->advance.x == one->glyph->advance.x * 2 && two->glyph->bitmap_top == one->glyph->bitmap_top * 2 && two->glyph->bitmap_left == one->glyph->bitmap_left * 2, "2x glyph metrics");
      for (unsigned y = 0; y < b.rows; ++y) for (unsigned x = 0; x < b.width; ++x)
        require(bool(b.buffer[y * b.pitch + x / 8] & (0x80 >> (x % 8))) == bool(a.buffer[(y / 2) * a.pitch + (x / 2) / 8] & (0x80 >> ((x / 2) % 8))), "Every source pixel becomes exactly a 2x2 block");
    }
    FT_Done_Face(one); FT_Done_Face(two); ++count;
  }
  require(count > 0, "BDF fixtures present");
  // User directories are discovered independently of the system's bitmap reject policy.
  auto temporary = std::filesystem::temp_directory_path() / ("dome-bdf-user-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
  auto user = temporary / "fonts"; std::filesystem::create_directories(user);
  setenv("XDG_DATA_HOME", temporary.c_str(), 1);
  auto before = bdf_fonts::discover(true);
  require(std::none_of(before.begin(),before.end(),[](auto &f){return f.family=="DomeUserBdf";}),"User font starts absent");
  auto source = bdf_fonts::read((std::filesystem::path(argv[1])/"spleen-v2.2.0/spleen-6x12.bdf").string());
  auto name = source.find("FAMILY_NAME "); auto end = source.find('\n',name);
  source.replace(name,end-name,R"(FAMILY_NAME "DomeUserBdf")");
  { std::ofstream out(user/"user.bdf"); out<<source; }
  auto compressed = source; name=compressed.find("FAMILY_NAME ");end=compressed.find('\n',name);
  compressed.replace(name,end-name,R"(FAMILY_NAME "DomeUserGzip")");
  auto gzip=gzopen((user/"user.bdf.gz").c_str(),"wb");require(gzip,"Create compressed BDF");gzwrite(gzip,compressed.data(),compressed.size());gzclose(gzip);
  auto after=bdf_fonts::discover(true);
  for(const char *family:{"DomeUserBdf","DomeUserGzip"}) {
    auto found=std::find_if(after.begin(),after.end(),[&](auto &f){return f.family==family;});
    require(found!=after.end(),"New system/user BDF discovered without restarting");
    require(bdf_fonts::resolve(family,found->style,found->pixels,2).pixels==found->pixels*2,"User BDF supports exact 2x");
  }
  auto outline=std::filesystem::path("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf");
  if(std::filesystem::exists(outline)) {
    std::filesystem::copy_file(outline,user/"outline.bdf");bdf_fonts::Face rejected;
    require(!bdf_fonts::inspect((user/"outline.bdf").string(),rejected),"Renamed outline font is rejected");
  }
  std::filesystem::remove_all(temporary);
  FT_Done_FreeType(library);
  std::cout << "PASS: " << count << " BDF files, native discovery and exact bdf2x glyph masks/advances\n";
} catch (const std::exception &e) { std::cerr << e.what() << '\n'; return 1; }
