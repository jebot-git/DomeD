// Native Wayland probe: request each new frame from the previous callback.
#define _GNU_SOURCE
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <time.h>
static struct wl_compositor *compositor;
static struct wl_shm *shm;
static struct xdg_wm_base *shell;
static struct wl_surface *surface;
static struct wl_buffer *buffer;
struct output { char name[128]; int refresh; };
static double stamp(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t); return t.tv_sec+t.tv_nsec/1e9; }
static void frame(void *data,struct wl_callback *cb,uint32_t time);
static const struct wl_callback_listener frames={frame};
static void request_frame(void) { struct wl_callback *cb=wl_surface_frame(surface); wl_callback_add_listener(cb,&frames,NULL); wl_surface_commit(surface); }
static void frame(void *data,struct wl_callback *cb,uint32_t time) {
 wl_callback_destroy(cb); printf("{\"kind\":\"frame\",\"time\":%.9f}\n",stamp()); request_frame();
}
static void ping(void *data,struct xdg_wm_base *base,uint32_t serial) { xdg_wm_base_pong(base,serial); }
static const struct xdg_wm_base_listener shell_events={ping};
static void configure(void *data,struct xdg_surface *xdg,uint32_t serial) {
 xdg_surface_ack_configure(xdg,serial);
 if (!buffer) {
  int fd=memfd_create("dome-refresh",MFD_CLOEXEC); ftruncate(fd,320*200*4);
  unsigned *p=mmap(NULL,320*200*4,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);
  for(int i=0;i<320*200;i++)p[i]=0xff4080bf;
  struct wl_shm_pool *pool=wl_shm_create_pool(shm,fd,320*200*4);
  buffer=wl_shm_pool_create_buffer(pool,0,320,200,320*4,WL_SHM_FORMAT_XRGB8888);
  wl_shm_pool_destroy(pool);close(fd);munmap(p,320*200*4);
  wl_surface_attach(surface,buffer,0,0);wl_surface_damage(surface,0,0,320,200);request_frame();
 }
}
static const struct xdg_surface_listener configure_events={configure};
static void geometry(void*d,struct wl_output*o,int32_t x,int32_t y,int32_t w,int32_t h,int32_t sub,const char*make,const char*model,int32_t transform){}
static void mode(void*d,struct wl_output*o,uint32_t flags,int32_t w,int32_t h,int32_t rate){if(flags&WL_OUTPUT_MODE_CURRENT)((struct output*)d)->refresh=rate;}
static void done(void*d,struct wl_output*o){struct output *s=d;printf("{\"kind\":\"output\",\"name\":\"%s\",\"refresh\":%d}\n",s->name,s->refresh);}
static void scale(void*d,struct wl_output*o,int32_t scale){}
static void name(void*d,struct wl_output*o,const char*n){snprintf(((struct output*)d)->name,128,"%s",n);}
static void description(void*d,struct wl_output*o,const char*n){}
static const struct wl_output_listener outputs={geometry,mode,done,scale,name,description};
static void global(void*d,struct wl_registry*r,uint32_t id,const char*iface,uint32_t version){
 if(!strcmp(iface,"wl_compositor"))compositor=wl_registry_bind(r,id,&wl_compositor_interface,4);
 else if(!strcmp(iface,"wl_shm"))shm=wl_registry_bind(r,id,&wl_shm_interface,1);
 else if(!strcmp(iface,"xdg_wm_base")){shell=wl_registry_bind(r,id,&xdg_wm_base_interface,1);xdg_wm_base_add_listener(shell,&shell_events,NULL);}
 else if(!strcmp(iface,"wl_output")){struct wl_output*o=wl_registry_bind(r,id,&wl_output_interface,version<4?version:4);wl_output_add_listener(o,&outputs,calloc(1,sizeof(struct output)));}
}
static void removed(void*d,struct wl_registry*r,uint32_t id){}
static const struct wl_registry_listener registry_events={global,removed};
int main(void){
 setvbuf(stdout,NULL,_IOLBF,0);struct wl_display*d=wl_display_connect(NULL);if(!d)return 1;
 struct wl_registry*r=wl_display_get_registry(d);wl_registry_add_listener(r,&registry_events,NULL);wl_display_roundtrip(d);
 if(!compositor||!shm||!shell)return 2;
 surface=wl_compositor_create_surface(compositor);
 struct xdg_surface*x=xdg_wm_base_get_xdg_surface(shell,surface);xdg_surface_add_listener(x,&configure_events,NULL);
 struct xdg_toplevel*top=xdg_surface_get_toplevel(x);xdg_toplevel_set_app_id(top,"dome-refresh-probe");xdg_toplevel_set_title(top,"Refresh probe");wl_surface_commit(surface);
 while(wl_display_dispatch(d)>=0){}return 0;
}
