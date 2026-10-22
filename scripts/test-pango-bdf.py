#!/usr/bin/env python3
"""Verify native BDF selection, widths and Cairo pixels through patched Pango.
Run with LD_LIBRARY_PATH pointing to the private Pango and uv --with freetype-py.
"""
import ctypes as C,pathlib,sys,freetype
fc=C.CDLL('libfontconfig.so.1');p=C.CDLL('libpango-1.0.so.0');pc=C.CDLL('libpangocairo-1.0.so.0');pf=C.CDLL('libpangoft2-1.0.so.0');c=C.CDLL('libcairo.so.2')
def signature(lib,name,args,result):
 fn=getattr(lib,name);fn.argtypes=args;fn.restype=result;return fn
ptr=C.c_void_p;integer=C.c_int;double=C.c_double;char=C.c_char_p
for lib,name,args,result in [(fc,'FcConfigAppFontAddFile',[ptr,char],integer),(fc,'FcConfigBuildFonts',[ptr],integer),(fc,'FcPatternGetString',[ptr,char,integer,C.POINTER(char)],integer),(c,'cairo_image_surface_create',[integer,integer,integer],ptr),(c,'cairo_create',[ptr],ptr),(c,'cairo_set_source_rgb',[ptr,double,double,double],None),(c,'cairo_paint',[ptr],None),(c,'cairo_image_surface_get_data',[ptr],C.POINTER(C.c_ubyte)),(c,'cairo_surface_flush',[ptr],None),(c,'cairo_destroy',[ptr],None),(c,'cairo_surface_destroy',[ptr],None),(pc,'pango_cairo_create_layout',[ptr],ptr),(pc,'pango_cairo_show_layout',[ptr,ptr],None),(p,'pango_layout_set_text',[ptr,char,integer],None),(p,'pango_layout_get_pixel_size',[ptr,C.POINTER(integer),C.POINTER(integer)],None),(p,'pango_layout_get_context',[ptr],ptr),(p,'pango_font_map_load_font',[ptr,ptr,ptr],ptr),(p,'pango_context_get_font_map',[ptr],ptr),(p,'pango_font_description_new',[],ptr),(p,'pango_font_description_set_family',[ptr,char],None),(p,'pango_font_description_set_absolute_size',[ptr,double],None),(p,'pango_font_description_set_weight',[ptr,integer],None),(p,'pango_layout_set_font_description',[ptr,ptr],None),(pf,'pango_fc_font_get_pattern',[ptr],ptr)]:signature(lib,name,args,result)
fonts=[pathlib.Path(f).resolve() for f in sys.argv[1:]];assert fonts
for file in fonts:assert fc.FcConfigAppFontAddFile(None,str(file).encode())
fc.FcConfigBuildFonts(None)
def crop(points):
 if not points:return []
 x=min(a for a,b in points);y=min(b for a,b in points);return sorted((a-x,b-y) for a,b in points)
for file in fonts:
 face=freetype.Face(str(file));face.select_size(0);size=round(face.available_sizes[0].y_ppem/64)
 surface=c.cairo_image_surface_create(0,1024,384);cr=c.cairo_create(surface);c.cairo_set_source_rgb(cr,0,0,0);c.cairo_paint(cr);layout=pc.pango_cairo_create_layout(cr)
 desc=p.pango_font_description_new();p.pango_font_description_set_family(desc,face.family_name);p.pango_font_description_set_absolute_size(desc,size*1024);p.pango_font_description_set_weight(desc,700 if face.style_flags&2 else 400);p.pango_layout_set_font_description(layout,desc);p.pango_layout_set_text(layout,b'MMMM',4)
 width=integer();height=integer();p.pango_layout_get_pixel_size(layout,C.byref(width),C.byref(height));face.load_char('M',freetype.FT_LOAD_DEFAULT);assert width.value==4*face.glyph.advance.x/64,(file,width.value,face.glyph.advance.x)
 context=p.pango_layout_get_context(layout);font=p.pango_font_map_load_font(p.pango_context_get_font_map(context),context,desc);assert font,'Pango rejected BDF';pattern=pf.pango_fc_font_get_pattern(font);actual=char();fc.FcPatternGetString(pattern,b'file',0,C.byref(actual));assert pathlib.Path(actual.value.decode()).resolve()==file,(file,actual.value)
 p.pango_layout_set_text(layout,b'M',1);c.cairo_set_source_rgb(cr,1,1,1);pc.pango_cairo_show_layout(cr,layout);c.cairo_surface_flush(surface);data=c.cairo_image_surface_get_data(surface);actual=[]
 for y in range(384):
  for x in range(1024):
   v=data[(y*1024+x)*4];assert v in (0,255),'Antialiased bitmap'
   if v:actual.append((x,y))
 bitmap=face.glyph.bitmap;expected=[(x,y) for y in range(bitmap.rows) for x in range(bitmap.width) if bitmap.buffer[y*bitmap.pitch+x//8]&(128>>(x%8))]
 assert crop(actual)==crop(expected),(file,'Cairo glyph pixels differ')
 c.cairo_destroy(cr);c.cairo_surface_destroy(surface);print('PASS raw BDF Pango/Cairo selection, metrics and exact pixels:',file.name,flush=True)
