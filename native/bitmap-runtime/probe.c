/* SPDX-License-Identifier: MIT */
#include <dlfcn.h>
#include <stdio.h>
int main(void) {
 const char *libs[]={"libfreetype.so.6","libcairo.so.2","libharfbuzz.so.0","libpangoft2-1.0.so.0"};
 const char *names[]={"FT_Dome_Bitmap_Only","cairo_dome_bitmap_only","hb_dome_bitmap_only","pango_dome_bitmap_only"};
 for(unsigned i=0;i<4;++i){
  void *lib=dlopen(libs[i],RTLD_NOW|RTLD_LOCAL); if(!lib){fprintf(stderr,"%s\n",dlerror());return 1;}
  int (*probe)(void)=(int (*)(void))dlsym(lib,names[i]);if(!probe||probe()!=1)return 1;
  if(i==0||i==2){unsigned (*formats)(void)=(unsigned (*)(void))dlsym(lib,i==0?"FT_Dome_Bitmap_Formats":"hb_dome_bitmap_formats");if(!formats||formats()!=3)return 1;}
 }
 puts("PASS FreeType/Cairo/HarfBuzz/Pango native BDF+PCF runtime probes");return 0;
}
