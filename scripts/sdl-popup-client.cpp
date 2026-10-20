// SDL3 native Wayland popup fixture: uses only SDL public popup APIs.
#include <SDL3/SDL.h>
#include <cstdio>
#include <string>

struct View { SDL_Window *window=nullptr; SDL_Renderer *renderer=nullptr; };
static View top, menu, nested;
static int sequence, selected;
static bool running=true;
static void destroy(View &v) {
    if(v.renderer) SDL_DestroyRenderer(v.renderer);
    if(v.window) SDL_DestroyWindow(v.window);
    v={};
}
static void close_menus() { destroy(nested); destroy(menu); selected=0; }
static void action(const char *name) {
    std::string title="SDL "+std::string(name)+" "+std::to_string(++sequence);
    SDL_SetWindowTitle(top.window,title.c_str());
    std::printf("ACTION %s\n",title.c_str()); std::fflush(stdout); close_menus();
}
static View popup(SDL_Window *parent, int x,int y,int w,int h) {
    View v; v.window=SDL_CreatePopupWindow(parent,x,y,w,h,SDL_WINDOW_POPUP_MENU);
    if(!v.window) { SDL_Log("Popup failed: %s",SDL_GetError()); return v; }
    v.renderer=SDL_CreateRenderer(v.window,nullptr);
    if(!v.renderer) { SDL_Log("Renderer failed: %s",SDL_GetError());destroy(v); }
    return v;
}
static void open_menu() {
    close_menus();selected=-1;menu=popup(top.window,40,96,210,128);
}
static void open_nested() {
    if(!nested.window && menu.window) nested=popup(menu.window,210,32,180,32);
}
static void choose(int row) {
    if(row==0) action("activate");
    else if(row==1) open_nested();
    else if(row==3) action("toggle");
}
static void paint(View &v, int kind) {
    if(!v.renderer)return;
    SDL_SetRenderDrawColor(v.renderer,kind?35:55,kind?70:55,kind?95:65,255);SDL_RenderClear(v.renderer);
    SDL_SetRenderDrawColor(v.renderer,240,240,240,255);
    if(kind==0) { SDL_FRect r{40,60,180,36};SDL_RenderRect(v.renderer,&r);SDL_RenderDebugText(v.renderer,60,74,"Open menu"); }
    else if(kind==1) {
        const char *labels[]={"Activate","Nested >","Disabled","Toggle"};
        for(int i=0;i<4;++i) { SDL_SetRenderDrawColor(v.renderer,i==2?110:240,240,240,255);SDL_RenderDebugText(v.renderer,12,float(i*32+12),labels[i]); }
    } else SDL_RenderDebugText(v.renderer,12,12,"Nested action");
    SDL_RenderPresent(v.renderer);
}
int main() {
    if(!SDL_Init(SDL_INIT_VIDEO)) { SDL_Log("%s",SDL_GetError());return 1; }
    SDL_SetAppMetadata("Dome SDL popup fixture","1","org.domed.SdlPopupFixture");
    top.window=SDL_CreateWindow("SDL fixture",480,320,0);
    if(!top.window) {SDL_Log("%s",SDL_GetError());return 1;}
    top.renderer=SDL_CreateRenderer(top.window,nullptr);
    if(!top.renderer) {SDL_Log("%s",SDL_GetError());return 1;}
    std::printf("DRIVER %s\n",SDL_GetCurrentVideoDriver()); std::fflush(stdout);
    while(running) {
        SDL_Event e;
        while(SDL_PollEvent(&e)) {
            if(e.type==SDL_EVENT_QUIT) running=false;
            if(e.type==SDL_EVENT_WINDOW_CLOSE_REQUESTED) {
                if(e.window.windowID==SDL_GetWindowID(top.window)) running=false;
                else if(menu.window && e.window.windowID==SDL_GetWindowID(menu.window))close_menus();
                else if(nested.window && e.window.windowID==SDL_GetWindowID(nested.window))destroy(nested);
            }
            if(e.type==SDL_EVENT_MOUSE_MOTION && menu.window && e.motion.windowID==SDL_GetWindowID(menu.window)) {
                selected=int(e.motion.y)/32;
                if(selected==1)open_nested();
            }
            if(e.type==SDL_EVENT_MOUSE_BUTTON_UP) {
                if(nested.window && e.button.windowID==SDL_GetWindowID(nested.window))action("nested");
                else if(menu.window && e.button.windowID==SDL_GetWindowID(menu.window))choose(int(e.button.y)/32);
                else if(e.button.windowID==SDL_GetWindowID(top.window) && e.button.y>=60 && e.button.y<96)open_menu();
            }
            if(e.type==SDL_EVENT_KEY_DOWN) {
                if(e.key.key==SDLK_ESCAPE)close_menus();
                else if(e.key.key==SDLK_M)open_menu();
                else if(menu.window && e.key.key==SDLK_DOWN)selected=(selected+1)%4;
                else if(menu.window && e.key.key==SDLK_UP)selected=(selected+3)%4;
                else if(menu.window && e.key.key==SDLK_RETURN) {if(nested.window)action("nested");else choose(selected);}
            }
        }
        paint(top,0);paint(menu,1);paint(nested,2);SDL_Delay(10);
    }
    close_menus();destroy(top);SDL_Quit();return 0;
}
