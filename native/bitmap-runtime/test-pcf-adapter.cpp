// SPDX-License-Identifier: MIT
#include "../bdf_font.hpp"
#include <iostream>
int main(int argc,char **argv) try {
 if(argc!=3)return 2;
 auto bytes=bdf_fonts::read(argv[1]);{std::ofstream out(argv[2]);out<<bytes;}
 FT_Library library;FT_Init_FreeType(&library);FT_Face original,converted;
 if(FT_New_Face(library,argv[1],0,&original)||FT_New_Face(library,argv[2],0,&converted))throw std::runtime_error("decode");
 if(std::string(original->style_name)!=converted->style_name)throw std::runtime_error("host style identity");
 FT_Select_Charmap(original,FT_ENCODING_UNICODE);FT_Select_Charmap(converted,FT_ENCODING_UNICODE);FT_Select_Size(original,0);FT_Select_Size(converted,0);
 if(original->size->metrics.ascender!=converted->size->metrics.ascender||original->size->metrics.descender!=converted->size->metrics.descender)throw std::runtime_error("font metrics");
 FT_UInt glyph;unsigned count=0;
 for(auto cp=FT_Get_First_Char(original,&glyph);glyph;cp=FT_Get_Next_Char(original,cp,&glyph)){
 auto cg=FT_Get_Char_Index(converted,cp);if(!cg||FT_Load_Glyph(original,glyph,FT_LOAD_DEFAULT)||FT_Load_Glyph(converted,cg,FT_LOAD_DEFAULT))throw std::runtime_error("coverage");
 auto a=original->glyph,b=converted->glyph;
 if(a->advance.x!=b->advance.x||a->bitmap_left!=b->bitmap_left||a->bitmap_top!=b->bitmap_top||a->bitmap.width!=b->bitmap.width||a->bitmap.rows!=b->bitmap.rows)throw std::runtime_error("glyph metrics");
 for(unsigned row=0;row<a->bitmap.rows;++row)for(unsigned col=0;col<a->bitmap.width;++col){auto bit=128>>(col%8);if((a->bitmap.buffer[row*a->bitmap.pitch+col/8]&bit)!=(b->bitmap.buffer[row*b->bitmap.pitch+col/8]&bit))throw std::runtime_error("glyph pixels");}
 ++count;
 }
 std::cout<<"PASS lossless PCF adapter: "<<count<<" Unicode glyphs and native metrics\n";
 bdf_fonts::Face source;if(!bdf_fonts::inspect(argv[1],source))throw std::runtime_error("inspect PCF");
 for(int scale:{1,2}){auto selected=bdf_fonts::scaled(source,scale);auto alias=bdf_fonts::qt_alias(selected,"browser");auto media=bdf_fonts::qt_alias(selected,"media");auto configuration=bdf_fonts::export_config();
 auto config=FcConfigCreate();if(!FcConfigParseAndLoad(config,(const FcChar8*)configuration.c_str(),FcTrue)||!FcConfigBuildFonts(config))throw std::runtime_error("exported font configuration");
 auto query=FcPatternCreate();FcPatternAddString(query,FC_FAMILY,(const FcChar8*)media.family.c_str());FcPatternAddDouble(query,FC_PIXEL_SIZE,media.pixels);FcConfigSubstitute(config,query,FcMatchPattern);FcDefaultSubstitute(query);FcResult result;auto match=FcFontMatch(config,query,&result);FcChar8 *file=nullptr;
 if(!match||FcPatternGetString(match,FC_FILE,0,&file)!=FcResultMatch||std::filesystem::path((const char*)file)!=media.file)throw std::runtime_error("exported media font identity");
 FcPatternDestroy(match);FcPatternDestroy(query);FcConfigDestroy(config);
 if(alias.pixels!=source.pixels*scale||alias.family.rfind("Dome Browser Bitmap ",0)!=0)throw std::runtime_error("selected strike");std::cout<<"PASS selected host PCF "<<scale<<"x browser alias\n";}
 FT_Done_Face(original);FT_Done_Face(converted);FT_Done_FreeType(library);
} catch(const std::exception &error){std::cerr<<error.what()<<"\n";return 1;}
