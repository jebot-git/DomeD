/* SPDX-License-Identifier: MIT */
#include <cairo.h>
#include <cairo-ft.h>
#include <cairo-xlib.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/XShm.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
static void require(int ok,const char *message){if(!ok){fprintf(stderr,"FAIL %s\n",message);exit(1);}}
int main(void){
 require(cairo_dome_bitmap_only(),"Canonical bitmap-only policy remains active");
 for(unsigned cycle=0;cycle<4;cycle++){
  Display *display=XOpenDisplay(NULL);require(display!=NULL,"Private X display opens");
  int major,minor;Bool pixmaps;require(XShmQueryVersion(display,&major,&minor,&pixmaps),"Actual MIT-SHM is available");
  int screen=DefaultScreen(display);Pixmap pixmap=XCreatePixmap(display,RootWindow(display,screen),512,512,DefaultDepth(display,screen));
  cairo_surface_t *surface=cairo_xlib_surface_create(display,pixmap,DefaultVisual(display,screen),512,512);
  cairo_surface_t *image=cairo_surface_create_similar_image(surface,CAIRO_FORMAT_ARGB32,512,512);
  require(cairo_surface_status(image)==CAIRO_STATUS_SUCCESS&&cairo_surface_get_device(image)==cairo_surface_get_device(surface),"Public similar-image API creates an actual device-owned SHM image");
  uint32_t *data=(uint32_t*)cairo_image_surface_get_data(image);int stride=cairo_image_surface_get_stride(image)/4;
  require(data!=NULL,"Actual SHM pixels are writable");
  for(unsigned y=0;y<512;y++)for(unsigned x=0;x<512;x++)data[y*stride+x]=0xff000000u|((x&255)<<16)|((y&255)<<8)|((x+y)&255);
  cairo_surface_mark_dirty(image);
  cairo_t *cr=cairo_create(surface);cairo_set_operator(cr,CAIRO_OPERATOR_SOURCE);cairo_set_source_surface(cr,image,0,0);cairo_paint(cr);
  cairo_surface_flush(surface);XSync(display,False);
  XImage *pixels=XGetImage(display,pixmap,0,0,512,512,AllPlanes,ZPixmap);require(pixels!=NULL,"Independent X pixels are readable");
  uint64_t hash=1469598103934665603ULL;
  for(unsigned y=0;y<512;y++)for(unsigned x=0;x<512;x++){unsigned long pixel=XGetPixel(pixels,x,y);for(unsigned b=0;b<4;b++){hash^=(pixel>>(b*8))&255;hash*=1099511628211ULL;}}
  XDestroyImage(pixels);printf("PIXELS %u %016llx\n",cycle,(unsigned long long)hash);fflush(stdout);
  /* Queue fresh server use after the independent read. Finish the device
   * before a synchronizing XCloseDisplay can retire the deferred record. */
  cairo_paint(cr);require(cairo_status(cr)==CAIRO_STATUS_SUCCESS,"Final actual SHM paint succeeds");
  cairo_device_t *device=cairo_device_reference(cairo_surface_get_device(surface));
  cairo_destroy(cr);cairo_surface_destroy(image);
  cairo_device_finish(device);cairo_device_finish(device);
  cairo_surface_destroy(surface);XFreePixmap(display,pixmap);XCloseDisplay(display);cairo_device_destroy(device);
 }
 puts("PASS four actual deferred-SHM display lifetimes");return 0;
}
