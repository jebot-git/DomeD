/* SPDX-License-Identifier: MIT */
#include <cairo.h>
#include <cairo-ft.h>
#include <cairo-xlib.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/Xrender.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
static void require(int ok,const char *why){if(!ok){fprintf(stderr,"FAIL %s\n",why);exit(1);}}
int main(void){
 require(cairo_dome_bitmap_only(),"Actual tested Cairo preserves the canonical bitmap-only policy");
 for(unsigned cycle=0;cycle<4;cycle++){
  Display *d=XOpenDisplay(NULL);require(d!=NULL,"Private X display opens");
  int major=0,minor=0;require(XRenderQueryVersion(d,&major,&minor),"Private display exposes actual Render");
  int screen=DefaultScreen(d);Pixmap p=XCreatePixmap(d,RootWindow(d,screen),128,128,DefaultDepth(d,screen));
  cairo_surface_t *surface=cairo_xlib_surface_create(d,p,DefaultVisual(d,screen),128,128);cairo_t *cr=cairo_create(surface);
  cairo_surface_t *mask=cairo_image_surface_create(CAIRO_FORMAT_A8,8,8);unsigned char *data=cairo_image_surface_get_data(mask);int stride=cairo_image_surface_get_stride(mask);
  for(unsigned y=0;y<8;y++)for(unsigned x=0;x<8;x++)data[y*stride+x]=(unsigned char)(32+((x+y)%4)*64);cairo_surface_mark_dirty(mask);
  cairo_set_source_rgb(cr,.25,.25,.25);cairo_paint(cr);
  for(unsigned alpha=0;alpha<256;alpha++){
   cairo_set_source_rgba(cr,0,0,0,alpha/255.);cairo_mask_surface(cr,mask,(alpha%16)*8,(alpha/16)*8);
  }
  for(unsigned i=0;i<96;i++){
   cairo_set_source_rgba(cr,(i%13)/12.,(i%7)/6.,(i%17)/16.,i<48?1:.5);
   cairo_mask_surface(cr,mask,(i%16)*8,(i/16)*8);
  }
  cairo_set_source_rgb(cr,1,1,1);cairo_mask_surface(cr,mask,0,0);
  require(cairo_status(cr)==CAIRO_STATUS_SUCCESS&&cairo_surface_status(surface)==CAIRO_STATUS_SUCCESS,"Actual source/mask rendering succeeds");
  cairo_surface_flush(surface);XSync(d,False);XImage *image=XGetImage(d,p,0,0,128,128,AllPlanes,ZPixmap);require(image!=NULL,"Independent X pixels are readable");
  uint64_t hash=1469598103934665603ULL;
  for(unsigned y=0;y<128;y++)for(unsigned x=0;x<128;x++){unsigned long pixel=XGetPixel(image,x,y);for(unsigned b=0;b<4;b++){hash^=(pixel>>(b*8))&255;hash*=1099511628211ULL;}}
  XDestroyImage(image);printf("PIXELS %u %016llx\n",cycle,(unsigned long long)hash);fflush(stdout);
  cairo_device_t *device=cairo_device_reference(cairo_surface_get_device(surface));
  if(cycle%2){cairo_device_finish(device);cairo_device_finish(device);}
  cairo_destroy(cr);cairo_surface_destroy(mask);cairo_surface_destroy(surface);XFreePixmap(d,p);XCloseDisplay(d);cairo_device_destroy(device);
 }
 puts("PASS four actual cached-source display lifetimes, including repeated explicit finish");return 0;
}
