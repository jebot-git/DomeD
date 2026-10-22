// SPDX-License-Identifier: MIT
#include "../fltk_appearance.h"
#include "notifier.h"
#include <FL/Fl_Double_Window.H>
#include <FL/Fl_Text_Editor.H>
#include <FL/Fl_Button.H>
#include <FL/Fl_Secret_Input.H>
#include <FL/Fl_Box.H>
#include <gio/gio.h>
#include <functional>
#include <iostream>

namespace {
class Editor : public Fl_Text_Editor {
public:
  using Fl_Text_Editor::Fl_Text_Editor;
  std::function<void(bool)> paste;
  std::function<void()> translate;
  int handle(int event) override {
    if (event == FL_PUSH && Fl::event_button() == FL_MIDDLE_MOUSE) {
      take_focus(); paste(true); return 1;
    }
    if ((event == FL_KEYDOWN || event == FL_SHORTCUT) && (Fl::event_state() & FL_CTRL)) {
      int key=Fl::event_key();
      if (key=='v') { paste(bool(Fl::event_state() & FL_SHIFT)); return 1; }
      if (key==FL_Enter || key==FL_KP_Enter) {translate(); return 1;}
    }
    return Fl_Text_Editor::handle(event);
  }
};
struct App {
  Fl_Double_Window window{960, 850, "DomeD Translator"};
  Fl_Double_Window settings{650, 220, "DeepL API Free settings"};
  Fl_Text_Buffer input, output, sourceReading, reading;
  Fl_Choice *source, *target;
  Editor *editor;
  Fl_Text_Display *result, *sourceView, *readingView;
  Fl_Button *translate;
  Fl_Box *status, *privacy;
  Fl_Secret_Input *key;
  std::vector<std::string> sourceCodes, targetCodes;
  std::unique_ptr<Notifier> notifier;
  std::string worker;
  bool quit=false, translating=false, updating=false;
  unsigned revision=0;
  unsigned jobs=0;
  struct Job { App *app; std::string action; unsigned revision; };

  void message(const std::string &text) { status->copy_label(text.c_str()); status->redraw(); }
  static Fl_Button *button(int x,int y,int w,const char *label,std::function<void()> fn) {
    auto b=new Fl_Button(x,y,w,30,label);
    auto callback=new std::function<void()>(std::move(fn));
    // Callbacks have process lifetime, matching the application widgets.
    b->callback([](Fl_Widget*,void*p){(*static_cast<std::function<void()>*>(p))();},callback);
    return b;
  }
  void request(Json task) {
    auto job=new Job{this,task.at("action"),revision};
    const char *argv[]={"/usr/bin/python3","-B",worker.c_str(),nullptr};
    GError *error=nullptr;
    auto process=g_subprocess_newv(argv,GSubprocessFlags(G_SUBPROCESS_FLAGS_STDIN_PIPE |
                    G_SUBPROCESS_FLAGS_STDOUT_PIPE | G_SUBPROCESS_FLAGS_STDERR_PIPE),&error);
    if(!process) {
      message("Could not start the translator worker.");
      if(error)g_error_free(error);
      if(job->action=="translate"){translating=false;translate->activate();}
      delete job; return;
    }
    ++jobs;
    auto data=task.dump();
    g_subprocess_communicate_utf8_async(process,data.c_str(),nullptr,
      [](GObject*o,GAsyncResult*r,gpointer userdata){
        std::unique_ptr<Job> j(static_cast<Job*>(userdata));
        gchar *out=nullptr,*err=nullptr;GError *error=nullptr;
        bool ok=g_subprocess_communicate_utf8_finish(G_SUBPROCESS(o),r,&out,&err,&error);
        Json response;
        try {
          if(!ok || !g_subprocess_get_successful(G_SUBPROCESS(o)))throw std::runtime_error("worker");
          response=Json::parse(out ? out : "");
        } catch(...) {response={{"ok",false},{"error","Translator worker failed. Check installation."}};}
        g_free(out);g_free(err);if(error)g_error_free(error);
        --j->app->jobs;
        if(!j->app->quit) j->app->complete(*j,response);
      },job);
    // The async operation owns a reference until completion.
    g_object_unref(process);
  }
  void setLanguages(const Json &data) {
    std::string oldSource=code(source,sourceCodes),oldTarget=code(target,targetCodes);
    source->clear();target->clear();sourceCodes={""};targetCodes.clear();source->add("Detect language");
    int si=0,ti=0;
    for(const auto &item:data.at("languages")) {
      std::string c=item.at("code"),name=item.at("name");
      bool s=item.value("source",true),t=item.value("target",true);
      auto label=name + ((!s||!t)&&data.value("api_ready",false) ? " (DeepL unavailable)" : "");
      source->add(label.c_str(),0,nullptr,nullptr,s?0:FL_MENU_INACTIVE);
      sourceCodes.push_back(c);
      target->add(label.c_str(),0,nullptr,nullptr,t?0:FL_MENU_INACTIVE);
      targetCodes.push_back(c);
      if(c==oldSource)si=int(sourceCodes.size())-1;
      if(c==oldTarget)ti=int(targetCodes.size())-1;
    }
    source->value(si);target->value(targetCodes.empty()?-1:ti);
    changed();
    std::string notice=data.value("notice","");
    if(notice.empty())notice="Ready. Ctrl+Enter translates; Ctrl+Shift+V pastes primary selection.";
    message(notice);
  }
  static std::string code(Fl_Choice*c,const std::vector<std::string>&codes) {
    auto n=c->value();return n>=0 && size_t(n)<codes.size() ? codes[n] : "";
  }
  void complete(const Job&job,const Json&data) {
    if(job.action=="translate") {translating=false;translate->activate();}
    if(!data.value("ok",false)){message(data.value("error","Translator request failed."));return;}
    if(job.action=="copy-primary")message("Copied to primary selection.");
    else if(job.action=="languages")setLanguages(data);
    else if(job.action=="save-key") {
      key->value("");settings.hide();
      message(data.value("environment_key",false)?"DEEPL_API_KEY takes precedence over the saved key.":"API key setting saved.");
      request({{"action","languages"}});
    } else if(job.action=="paste") {
      if(job.revision!=revision){message("Input changed while reading selection; paste again.");return;}
      // Like normal editor paste, replace the selection or insert at the caret.
      int start=0,end=0;
      if(input.selection_position(&start,&end)){editor->insert_position(start);input.remove_selection();}
      editor->insert(data.at("paste").get<std::string>().c_str());
      editor->show_insert_position();editor->take_focus();
      message("Text pasted. Press Translate to send it to DeepL.");
    } else if(job.action=="translate") {
      if(job.revision!=revision){message("Input or language changed. Press Translate again.");return;}
      updating=true;
      output.text(data.at("text").get<std::string>().c_str());
      reading.text(data.value("reading","").c_str());
      sourceReading.text(data.value("source_reading","").c_str());
      updating=false;
      message(data.value("notice","").empty() ? "Translated. Detected source: " + data.value("detected","") : data.value("notice",""));
    }
  }
  void changed() {
    if(updating)return;
    ++revision;output.text("");reading.text("");sourceReading.text("");
  }
  void paste(bool primary) {
    window.show();
    if(getenv("WAYLAND_DISPLAY"))request({{"action","paste"},{"primary",primary}});
    else Fl::paste(*editor,primary?0:1);
  }
  void copy(Fl_Text_Buffer&buffer) {
    char *text=buffer.text();Fl::copy(text,int(strlen(text)),1);free(text);message("Copied to clipboard.");
  }
  void copyPrimary(Fl_Text_Buffer&buffer) {
    char *text=buffer.text();
    if(getenv("WAYLAND_DISPLAY"))request({{"action","copy-primary"},{"text",text}});
    else {Fl::copy(text,int(strlen(text)),0);message("Copied to primary selection.");}
    free(text);
  }
  void startTranslation() {
    if(translating)return;
    char *text=input.text();std::string value=text;free(text);
    auto dst=code(target,targetCodes);
    if(dst.empty()){message("No target language is configured. Refresh input languages.");return;}
    if(value.find_first_not_of(" \t\r\n")==std::string::npos){message("Enter or paste text first.");return;}
    translating=true;translate->deactivate();message("Translating with DeepL API Free…");
    request({{"action","translate"},{"text",value},{"source",code(source,sourceCodes)},{"target",dst}});
  }
  void action(const std::string&a) {
    if(a=="quit"){quit=true;settings.hide();window.hide();}
    else if(a=="settings"){window.show();settings.show();}
    else if(a=="clipboard")paste(false);
    else if(a=="primary")paste(true);
    else {window.show();editor->take_focus();}
  }
  ~App() {
    // Displays must detach before the member text buffers are destroyed.
    editor->buffer(nullptr);result->buffer(nullptr);sourceView->buffer(nullptr);readingView->buffer(nullptr);
  }
  App() {
    auto executable=std::filesystem::read_symlink("/proc/self/exe");
    auto installed=executable.parent_path().parent_path()/"libexec/dome-translator/backend.py";
    worker=std::filesystem::exists(installed)?installed.string():std::string(TRANSLATOR_BUILD_DIR)+"/backend.py";
    window.begin();
    source=new Fl_Choice(95,16,300,30,"From:");
    target=new Fl_Choice(490,16,300,30,"To:");
    button(805,16,140,"Settings",[this]{action("settings");});
    source->callback([](Fl_Widget*,void*p){static_cast<App*>(p)->changed();},this);
    target->callback([](Fl_Widget*,void*p){static_cast<App*>(p)->changed();},this);
    button(15,58,155,"Paste clipboard",[this]{paste(false);});
    button(180,58,190,"Paste primary selection",[this]{paste(true);});
    button(380,58,160,"Refresh languages",[this]{request({{"action","languages"}});});
    translate=button(770,58,175,"Translate",[this]{startTranslation();});
    editor=new Editor(15,116,930,155,"Input");editor->align(FL_ALIGN_TOP_LEFT);editor->buffer(&input);
    editor->wrap_mode(Fl_Text_Display::WRAP_AT_BOUNDS,0);
    editor->paste=[this](bool primary){paste(primary);};editor->translate=[this]{startTranslation();};
    sourceView=new Fl_Text_Display(15,300,930,67,"Input transcription (Japanese / Korean and other non-Latin scripts)");
    sourceView->align(FL_ALIGN_TOP_LEFT);sourceView->buffer(&sourceReading);sourceView->wrap_mode(Fl_Text_Display::WRAP_AT_BOUNDS,0);
    result=new Fl_Text_Display(15,397,930,155,"Translation — original script");
    result->align(FL_ALIGN_TOP_LEFT);result->buffer(&output);result->wrap_mode(Fl_Text_Display::WRAP_AT_BOUNDS,0);
    button(15,563,160,"Copy translation",[this]{copy(output);});
    button(185,563,210,"Copy to primary selection",[this]{copyPrimary(output);});
    readingView=new Fl_Text_Display(15,624,930,82,"Translation transcription (blank for Latin / Cyrillic)");
    readingView->align(FL_ALIGN_TOP_LEFT);readingView->buffer(&reading);readingView->wrap_mode(Fl_Text_Display::WRAP_AT_BOUNDS,0);
    button(15,716,180,"Copy transcription",[this]{copy(reading);});
    button(205,716,190,"Copy input transcription",[this]{copy(sourceReading);});
    button(405,716,240,"Copy transcription to primary",[this]{copyPrimary(reading);});
    button(805,716,140,"Hide to tray",[this]{if(notifier)window.hide();else quit=true;});
    status=new Fl_Box(15,760,930,38,"Loading input languages…");status->align(FL_ALIGN_INSIDE|FL_ALIGN_LEFT|FL_ALIGN_WRAP);
    privacy=new Fl_Box(15,804,930,30,"Translate sends the input to DeepL API Free. Pasting alone stays local.");
    privacy->align(FL_ALIGN_INSIDE|FL_ALIGN_LEFT|FL_ALIGN_WRAP);
    window.end();window.resizable(editor);window.size_range(960,850);
    window.xclass("org.domed.Translator");
    window.callback([](Fl_Widget*,void*p){auto app=static_cast<App*>(p);if(app->notifier)app->window.hide();else app->quit=true;},this);
    input.add_modify_callback([](int,int inserted,int deleted,int,const char*,void*p){if(inserted||deleted)static_cast<App*>(p)->changed();},this);
    settings.begin();
    auto info=new Fl_Box(15,15,620,50,"DeepL API Free key (:fx). Stored privately in your user configuration.\nDEEPL_API_KEY, when set, takes precedence. The key is never shown here.");
    info->align(FL_ALIGN_INSIDE|FL_ALIGN_LEFT|FL_ALIGN_WRAP);
    key=new Fl_Secret_Input(100,85,530,30,"API key:");
    button(15,145,150,"Save key",[this]{request({{"action","save-key"},{"key",key->value()}});key->value("");});
    button(175,145,180,"Remove saved key",[this]{request({{"action","save-key"},{"key",""}});});
    button(480,145,150,"Cancel",[this]{key->value("");settings.hide();});
    settings.end();settings.set_non_modal();settings.callback([](Fl_Widget*w,void*p){static_cast<App*>(p)->key->value("");w->hide();},this);
    dx::style(&window);dx::style(&settings);
  }
};
}
int main(int argc,char **argv) {
  std::string action="open";bool tray=false,smoke=false;
  for(int i=1;i<argc;++i) {
    std::string a=argv[i];
    if(a=="--tray")tray=true;
    else if(a=="--primary")action="primary";
    else if(a=="--clipboard")action="clipboard";
    else if(a=="--smoke-test"){smoke=true;++i;}
    else if(a=="--help") {std::cout<<"dome-translator [--tray | --clipboard | --primary | --smoke-test FILE]\n";return 0;}
    else {std::cerr<<"Unknown option: "<<a<<'\n';return 2;}
  }
  if(!smoke && Notifier::showExisting(action))return 0;
  try {
    dx::init();App app;
    if(smoke){
      app.sourceCodes={"","en","cs","sr","ja","ko"};app.targetCodes={"en","cs","sr","ja","ko"};
      app.source->add("Detect language|English|Czech|Serbian|Japanese|Korean");app.source->value(1);
      app.target->add("English|Czech|Serbian|Japanese|Korean");app.target->value(3);
      app.input.text("Hello! Ahoj! Здраво!");app.output.text("こんにちは！");
      app.reading.text("konnichiha!");app.message("Preview — no text has been sent to DeepL.");
      dx::smoke(app.window,argc,argv);
    }
    try {app.notifier=std::make_unique<Notifier>([&](const std::string&a){app.action(a);});}
    catch(const std::exception&e) {if(Notifier::showExisting(action))return 0;std::cerr<<"Tray unavailable: "<<e.what()<<'\n';tray=false;}
    if(!tray)app.action(action);
    app.request({{"action","languages"}});
    // Fl::run exits when the last window hides; tray applications must keep polling.
    while(!app.quit) {while(g_main_context_iteration(nullptr,FALSE)){} Fl::wait(.02);}
    // Let workers finish before their App and buffers go away (bounded HTTP timeout).
    while(app.jobs){g_main_context_iteration(nullptr,TRUE);}
    return 0;
  } catch(const std::exception&e){std::cerr<<"DomeD Translator: "<<e.what()<<'\n';return 1;}
}
