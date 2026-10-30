// DomeD bitmap fork regression: real BDF metrics/shaping and outline rejection.
#include <hb.h>
#include <hb-ft.h>
#include <ft2build.h>
#include FT_FREETYPE_H
#include <cstring>
#include <iostream>
#include <stdexcept>
static void check(bool ok, const char *message) { if (!ok) throw std::runtime_error(message); }
static unsigned destroyed;
static void destroy_ft(void *p) { ++destroyed; FT_Done_Face(static_cast<FT_Face>(p)); }
static hb_blob_t *table(hb_face_t *, hb_tag_t, void *) { return hb_blob_get_empty(); }
static void destroy_int(void *p) { ++*static_cast<int *>(p); }
int main(int argc, char **argv) try {
  check(argc >= 3, "Usage: test-bdf OUTLINE.ttf FONT.bdf ...");
  FT_Library library; check(!FT_Init_FreeType(&library), "FreeType initialization");
  hb_blob_t *outline = hb_blob_create_from_file_or_fail(argv[1]);
  check(outline, "outline fixture required");
  check(!hb_face_count(outline), "outline accepted by face_count");
  hb_face_t *rejected = hb_face_create(outline, 0);
  check(!hb_face_get_glyph_count(rejected), "outline accepted by blob constructor");
  hb_face_destroy(rejected); hb_blob_destroy(outline);
  for (bool font : {false, true}) {
    FT_Face ft; check(!FT_New_Face(library, argv[1], 0, &ft), "load outline FT fixture");
    if (font) {
      hb_font_t *f = hb_ft_font_create(ft, destroy_ft);
      check(!hb_face_get_glyph_count(hb_font_get_face(f)), "outline accepted by FT font constructor");
      hb_font_destroy(f);
    } else {
      hb_face_t *f = hb_ft_face_create(ft, destroy_ft);
      check(!hb_face_get_glyph_count(f), "outline accepted by FT face constructor");
      hb_face_destroy(f);
    }
  }
  check(destroyed == 2, "rejected FT ownership callback");
  int calls = 0;
  hb_face_t *callback = hb_face_create_for_tables(table, &calls, destroy_int);
  check(!hb_face_get_glyph_count(callback) && calls == 1, "table callback bypass");
  hb_face_destroy(callback);
  hb_blob_t *bad = hb_blob_create("STARTFONT 2.1\n", 14, HB_MEMORY_MODE_READONLY, nullptr, nullptr);
  rejected = hb_face_create(bad, 0); check(!hb_face_get_glyph_count(rejected), "malformed BDF accepted");
  hb_face_destroy(rejected); hb_blob_destroy(bad);
  for (int i=2; i<argc; ++i) {
    FT_Face ft; check(!FT_New_Face(library, argv[i], 0, &ft), "load BDF");
    check(!FT_Select_Charmap(ft, FT_ENCODING_UNICODE) && !FT_Select_Size(ft, 0), "select BDF Unicode strike");
    hb_blob_t *blob = hb_blob_create_from_file_or_fail(argv[i]); check(blob, "BDF blob");
    check(hb_face_count(blob) == 1, "BDF face count");
    hb_face_t *face = hb_face_create(blob, 0); hb_blob_destroy(blob);
    check(hb_face_get_glyph_count(face) == static_cast<unsigned>(ft->num_glyphs), "native glyph count");
    check(hb_face_get_upem(face) == ft->size->metrics.y_ppem, "bitmap design units");
    hb_set_t *coverage = hb_set_create();
    hb_face_collect_unicodes(face, coverage);
    check(hb_set_has(coverage, 'M') && hb_set_has(coverage, 'A'), "BDF Unicode coverage");
    unsigned covered = 0; FT_UInt glyph_index;
    for (FT_ULong cp=FT_Get_First_Char(ft, &glyph_index); glyph_index;
         cp=FT_Get_Next_Char(ft, cp, &glyph_index)) {
      check(hb_set_has(coverage, cp), "missing native Unicode coverage"); ++covered;
    }
    check(hb_set_get_population(coverage) == covered, "coverage count differs");
    hb_set_destroy(coverage);
    for (bool from_ft : {false, true}) {
      hb_font_t *font = from_ft ? hb_ft_font_create(ft, nullptr) : hb_font_create(face);
      for (int scale : {1, 2}) {
        int ppem = ft->size->metrics.y_ppem;
        hb_font_set_scale(font, ppem * 1024 * scale, ppem * 1024 * scale);
        hb_codepoint_t m;
        check(hb_font_get_nominal_glyph(font, 'M', &m) && m == FT_Get_Char_Index(ft, 'M'), "native cmap");
        check(!FT_Load_Glyph(ft, m, FT_LOAD_DEFAULT), "load glyph metrics");
        check(hb_font_get_glyph_h_advance(font, m) == ft->glyph->advance.x * 16 * scale, "native advance");
        hb_glyph_extents_t extents;
        check(hb_font_get_glyph_extents(font, m, &extents), "native ink extents");
        check(extents.width == ft->glyph->metrics.width * 16 * scale &&
              extents.height == -ft->glyph->metrics.height * 16 * scale &&
              extents.x_bearing == ft->glyph->metrics.horiBearingX * 16 * scale &&
              extents.y_bearing == ft->glyph->metrics.horiBearingY * 16 * scale, "ink metrics differ");
        for (hb_direction_t direction : {HB_DIRECTION_LTR, HB_DIRECTION_RTL}) {
          hb_buffer_t *buffer = hb_buffer_create();
          hb_buffer_add_utf8(buffer, "M A", -1, 0, -1);
          hb_buffer_set_direction(buffer, direction); hb_buffer_guess_segment_properties(buffer);
          hb_shape(font, buffer, nullptr, 0);
          unsigned count; auto info = hb_buffer_get_glyph_infos(buffer, &count);
          auto positions = hb_buffer_get_glyph_positions(buffer, nullptr);
          check(count == 3, "BDF shaping lost glyphs");
          for (unsigned j=0; j<count; ++j) {
            unsigned cluster = direction == HB_DIRECTION_RTL ? 2-j : j;
            auto glyph = FT_Get_Char_Index(ft, "M A"[cluster]);
            check(info[j].codepoint == glyph && info[j].cluster == cluster, "glyph/cluster mapping");
            check(!FT_Load_Glyph(ft, glyph, FT_LOAD_DEFAULT), "load shaped glyph");
            check(positions[j].x_advance == ft->glyph->advance.x * 16 * scale, "shaped advance differs");
          }
          hb_buffer_destroy(buffer);
        }
      }
      hb_font_destroy(font);
    }
    hb_face_destroy(face); FT_Done_Face(ft);
    std::cout << "PASS native BDF blob/FreeType metrics, 1x/2x, LTR/RTL: " << argv[i] << '\n';
  }
  FT_Done_FreeType(library);
  std::cout << "PASS outline, table callback and malformed-font rejection; ownership callbacks\n";
  return 0;
} catch (const std::exception &e) { std::cerr << e.what() << '\n'; return 1; }
