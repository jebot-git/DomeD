// SPDX-License-Identifier: MIT
#define main translator_application_main
#include "../../native/translator/main.cpp"
#undef main
#include <chrono>
#include <thread>

static int checks=0;
static void check(bool value,const char *label) {
  if(!value)throw std::runtime_error(label);
  std::cout<<"PASS "<<label<<std::endl;++checks;
}
template<class F> static bool wait(F test,int milliseconds=5000) {
  auto until=std::chrono::steady_clock::now()+std::chrono::milliseconds(milliseconds);
  do {while(g_main_context_iteration(nullptr,FALSE)){}Fl::wait(.01);if(test())return true;}
  while(std::chrono::steady_clock::now()<until);
  return false;
}
static std::string contents(Fl_Text_Buffer&buffer){auto p=buffer.text();std::string s=p;free(p);return s;}
static void clipboard(const std::string&text,bool primary) {
  if(getenv("WAYLAND_DISPLAY")) {
    GError *error=nullptr;
    auto p=g_subprocess_new(GSubprocessFlags(G_SUBPROCESS_FLAGS_STDIN_PIPE),&error,
                "wl-copy","--type","text/plain;charset=utf-8",primary?"--primary":nullptr,nullptr);
    if(!p)throw std::runtime_error(error?error->message:"wl-copy");
    check(g_subprocess_communicate_utf8(p,text.c_str(),nullptr,nullptr,nullptr,&error),"seed isolated Wayland selection");
    g_object_unref(p);
  } else Fl::copy(text.c_str(),int(text.size()),primary?0:1);
}
int main() {
  try {
    dx::init();App app;app.window.show();
    app.notifier=std::make_unique<Notifier>([&](const std::string&a){app.action(a);});
    app.setLanguages({{"languages",Json::array({{{"code","en"},{"name","English"}},
        {{"code","ja"},{"name","Japanese"}},{{"code","cs"},{"name","Czech"}},
        {{"code","sr"},{"name","Serbian"}},{{"code","ko"},{"name","Korean"}}})}});
    clipboard("CLIPBOARD_Žluťoučký\n日本語",false);
    clipboard("PRIMARY_Здраво_한국어",true);
    app.paste(false);
    check(wait([&]{return contents(app.input)=="CLIPBOARD_Žluťoučký\n日本語";}),"clipboard paste preserves UTF-8 and newlines");
    app.input.text("");app.editor->insert_position(0);app.paste(true);
    check(wait([&]{return contents(app.input)=="PRIMARY_Здраво_한국어";}),"primary paste stays independent of clipboard");
    app.input.text("");app.editor->insert_position(0);app.paste(false);
    check(wait([&]{return contents(app.input)=="CLIPBOARD_Žluťoučký\n日本語";}),"primary paste leaves clipboard unchanged");
    app.input.select(0,9);app.editor->insert_position(9);app.paste(true);
    check(wait([&]{return contents(app.input)=="PRIMARY_Здраво_한국어_Žluťoučký\n日本語";}),"paste replaces editor selection");
    check(!app.translating,"pasting never starts translation");
    app.worker=TRANSLATOR_TEST_WORKER;app.target->value(1);app.input.text("hello");
    app.startTranslation();
    check(app.translating && !app.translate->active(),"translation disables duplicate submission");
    int ticks=0;
    while(app.jobs){while(g_main_context_iteration(nullptr,FALSE)){}Fl::wait(.01);++ticks;}
    check(ticks>=5,"FLTK remains responsive during worker request");
    check(contents(app.output)=="日本語" && contents(app.reading)=="nihongo","script and transcription occupy separate buffers");
    auto realWorker=std::string(TRANSLATOR_BUILD_DIR)+"/backend.py";
    app.worker=realWorker;app.copyPrimary(app.output);
    check(wait([&]{return app.jobs==0;}),"copy translation to primary completes locally");
    app.input.text("");app.editor->insert_position(0);app.paste(true);
    check(wait([&]{return contents(app.input)=="日本語";}),"translated script can be copied to primary selection");
    app.input.text("");app.editor->insert_position(0);app.paste(false);
    check(wait([&]{return contents(app.input)=="CLIPBOARD_Žluťoučký\n日本語";}),"copying primary preserves clipboard");
    app.worker=TRANSLATOR_TEST_WORKER;
    app.startTranslation();app.input.text("changed during request");
    check(wait([&]{return app.jobs==0;}),"pending translation completes");
    check(contents(app.output).empty(),"stale translation cannot overwrite changed input");
    check(!app.translating && app.translate->active(),"translation control recovers after stale result");
    app.window.hide();check(!app.window.shown(),"window can hide while tray remains alive");
    // A second process must activate this instance, not create another translator.
    auto p=g_subprocess_new(G_SUBPROCESS_FLAGS_NONE,nullptr,TRANSLATOR_TEST_BINARY,nullptr);
    bool exited=false;
    g_subprocess_wait_async(p,nullptr,[](GObject*o,GAsyncResult*r,gpointer data){
      g_subprocess_wait_finish(G_SUBPROCESS(o),r,nullptr);*static_cast<bool*>(data)=true;
    },&exited);
    check(wait([&]{return exited;}),"second instance exits after activation");
    check(g_subprocess_get_exit_status(p)==0 && app.window.shown(),"second instance reopens existing window");
    g_object_unref(p);
    // Exercise exported tray properties/menu using asynchronous self calls.
    auto bus=g_bus_get_sync(G_BUS_TYPE_SESSION,nullptr,nullptr);
    struct Reply {GVariant* value=nullptr;bool done=false;};
    auto call=[&](const char*path,const char*iface,const char*method,GVariant*args){
      Reply reply;
      g_dbus_connection_call(bus,"org.domed.Translator",path,iface,method,args,nullptr,
        G_DBUS_CALL_FLAGS_NONE,2000,nullptr,[](GObject*o,GAsyncResult*r,gpointer data){
          auto p=static_cast<Reply*>(data);p->value=g_dbus_connection_call_finish(G_DBUS_CONNECTION(o),r,nullptr);p->done=true;
        },&reply);
      check(wait([&]{return reply.done;}),"tray D-Bus reply arrives without blocking UI");
      return reply.value;
    };
    auto props=call("/Translator","org.freedesktop.DBus.Properties","GetAll",g_variant_new("(s)","org.kde.StatusNotifierItem"));
    check(props!=nullptr,"StatusNotifier properties exported");
    auto dict=g_variant_get_child_value(props,0);auto pix=g_variant_lookup_value(dict,"IconPixmap",G_VARIANT_TYPE("a(iiay)"));
    auto id=g_variant_lookup_value(dict,"Id",G_VARIANT_TYPE_STRING);
    check(id && std::string(g_variant_get_string(id,nullptr))=="dome.translator","tray item has a stable translator identity");
    if(id)g_variant_unref(id);
    check(pix && g_variant_n_children(pix)==2,"tray has native 24px and 48px ARGB icons");
    if(pix)g_variant_unref(pix);
    g_variant_unref(dict);g_variant_unref(props);
    auto args=g_variant_new("(ii@as)",0,1,g_variant_new_strv(nullptr,0));
    auto layout=call("/Translator/Menu","com.canonical.dbusmenu","GetLayout",args);
    auto printable=g_variant_print(layout,FALSE);std::string menu=printable;g_free(printable);g_variant_unref(layout);
    check(menu.find("Paste primary selection")!=std::string::npos && menu.find("Quit")!=std::string::npos,"tray menu offers separate paste actions and quit");
    auto reply=call("/Translator/Menu","com.canonical.dbusmenu","Event",g_variant_new("(isvu)",5,"clicked",g_variant_new_int32(0),0u));
    if(reply)g_variant_unref(reply);
    check(app.quit,"tray quit action exits application");
    g_object_unref(bus);
    std::cout<<checks<<" integration checks passed\n";
    return 0;
  }catch(const std::exception&e){std::cerr<<"FAIL "<<e.what()<<'\n';return 1;}
}
