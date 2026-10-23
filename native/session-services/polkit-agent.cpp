// SPDX-License-Identifier: MIT
#include "gpu-window.h"
#include "login-session.h"
#include <FL/Fl_Box.H>
#include <FL/Fl_Button.H>
#include <FL/Fl_Choice.H>
#include <FL/Fl_Input.H>
#include <FL/Fl_Return_Button.H>
#define POLKIT_AGENT_I_KNOW_API_IS_SUBJECT_TO_CHANGE
#include <cstring>
#include <gio/gio.h>
#include <polkitagent/polkitagent.h>
#include <pwd.h>
#include <signal.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <vector>

struct AgentUI {
  dx::GpuWindow window{620, 330, "DomeD Authentication"};
  Fl_Box message{20, 16, 580, 70};
  Fl_Box action{20, 87, 580, 35};
  Fl_Choice identity{155, 130, 430, 30, "Authenticate as:"};
  Fl_Input secret{155, 178, 430, 32, "Password:"};
  Fl_Box status{20, 220, 580, 40};
  Fl_Button cancel{360, 278, 105, 32, "Cancel"};
  Fl_Return_Button submit{475, 278, 125, 32, "Authenticate"};
  GTask *task = nullptr;
  PolkitAgentSession *session = nullptr;
  GCancellable *cancellable = nullptr;
  gulong cancel_handler = 0;
  std::vector<PolkitIdentity *> identities;
  std::string cookie;
  bool cancelled = false, changing = false;
  unsigned attempts = 0, generation = 0;
  AgentUI() {
    window.end();
    window.set_modal();
    message.align(FL_ALIGN_INSIDE | FL_ALIGN_LEFT | FL_ALIGN_WRAP);
    action.align(FL_ALIGN_INSIDE | FL_ALIGN_LEFT | FL_ALIGN_WRAP);
    status.align(FL_ALIGN_INSIDE | FL_ALIGN_LEFT | FL_ALIGN_WRAP);
    secret.type(FL_SECRET_INPUT);
    cancel.callback(
        [](Fl_Widget *, void *p) { static_cast<AgentUI *>(p)->abort(); }, this);
    window.callback(
        [](Fl_Widget *, void *p) { static_cast<AgentUI *>(p)->abort(); }, this);
    submit.callback(
        [](Fl_Widget *, void *p) { static_cast<AgentUI *>(p)->answer(); },
        this);
    identity.callback(
        [](Fl_Widget *, void *p) {
          auto ui = static_cast<AgentUI *>(p);
          if (ui->session) {
            ui->changing = true;
            polkit_agent_session_cancel(ui->session);
          } else
            ui->start();
        },
        this);
  }
  void wipe() {
    // FLTK owns this mutable, NUL-terminated input buffer. Wipe before
    // replacing it.
    auto value = const_cast<char *>(secret.value());
    if (value)
      explicit_bzero(value, strlen(value));
    secret.value("");
  }
  void finish(bool success, const char *why = "Authentication cancelled") {
    if (!task)
      return;
    window.hide();
    wipe();
    if (cancellable && cancel_handler)
      g_cancellable_disconnect(cancellable, cancel_handler);
    cancel_handler = 0;
    g_clear_object(&cancellable);
    if (session) {
      g_signal_handlers_disconnect_by_data(session, this);
      g_clear_object(&session);
    }
    for (auto i : identities)
      g_object_unref(i);
    identities.clear();
    explicit_bzero(cookie.data(), cookie.size());
    cookie.clear();
    auto result = task;
    task = nullptr;
    if (success)
      g_task_return_boolean(result, TRUE);
    else
      g_task_return_new_error(result, POLKIT_ERROR, POLKIT_ERROR_CANCELLED,
                              "%s", why);
    g_object_unref(result);
  }
  void abort() {
    cancelled = true;
    wipe();
    if (session)
      polkit_agent_session_cancel(session);
    else
      finish(false);
  }
  void answer() {
    if (!session) {
      start();
      return;
    }
    secret.deactivate();
    submit.deactivate();
    polkit_agent_session_response(session, secret.value());
    wipe();
  }
  void start() {
    if (!task || identities.empty())
      return;
    wipe();
    secret.deactivate();
    submit.deactivate();
    status.copy_label("Waiting for authentication…");
    session = polkit_agent_session_new(identities.at(identity.value()),
                                       cookie.c_str());
    g_signal_connect(session, "request",
                     reinterpret_cast<GCallback>(
                         +[](PolkitAgentSession *, const char *prompt,
                             gboolean echo, gpointer p) {
                           auto ui = static_cast<AgentUI *>(p);
                           ui->secret.type(echo ? FL_NORMAL_INPUT
                                                : FL_SECRET_INPUT);
                           ui->secret.copy_label(prompt);
                           ui->secret.activate();
                           ui->submit.activate();
                           ui->secret.take_focus();
                           ui->window.redraw();
                         }),
                     this);
    for (const char *signal : {"show-info", "show-error"})
      g_signal_connect(
          session, signal,
          reinterpret_cast<GCallback>(
              +[](PolkitAgentSession *, const char *text, gpointer p) {
                auto ui = static_cast<AgentUI *>(p);
                ui->status.copy_label(text);
                ui->window.redraw();
              }),
          this);
    g_signal_connect(
        session, "completed",
        reinterpret_cast<GCallback>(
            +[](PolkitAgentSession *s, gboolean gained, gpointer p) {
              auto ui = static_cast<AgentUI *>(p);
              g_signal_handlers_disconnect_by_data(s, ui);
              ui->session = nullptr;
              g_object_unref(s);
              if (ui->changing && !ui->cancelled) {
                ui->changing = false;
                ui->start();
                return;
              }
              if (gained) {
                ui->finish(true);
                return;
              }
              if (ui->cancelled) {
                ui->finish(false);
                return;
              }
              if (++ui->attempts >= 3) {
                ui->finish(false, "Authentication failed");
                return;
              }
              ui->status.copy_label(
                  "Authentication failed. Choose Authenticate to try again.");
              ui->submit.activate();
              ui->window.redraw();
            }),
        this);
    polkit_agent_session_initiate(session);
  }
  void begin(GObject *listener, const char *id, const char *text,
             const char *token, GList *allowed, GCancellable *cancel_token,
             GAsyncReadyCallback callback, gpointer data) {
    if (task) {
      auto pending = g_task_new(listener, cancel_token, callback, data);
      g_task_return_new_error(pending, G_IO_ERROR, G_IO_ERROR_PENDING,
                              "Another authentication is pending");
      g_object_unref(pending);
      return;
    }
    task = g_task_new(listener, cancel_token, callback, data);
    cancelled = changing = false;
    attempts = 0;
    ++generation;
    cookie = token;
    message.copy_label(text);
    action.copy_label(id);
    identity.clear();
    int selected = 0;
    for (auto l = allowed; l; l = l->next) {
      auto item = POLKIT_IDENTITY(l->data);
      identities.push_back(POLKIT_IDENTITY(g_object_ref(item)));
      char *name = polkit_identity_to_string(item);
      std::string label = name;
      g_free(name);
      if (POLKIT_IS_UNIX_USER(item)) {
        auto uid = polkit_unix_user_get_uid(POLKIT_UNIX_USER(item));
        if (auto account = getpwuid(uid))
          label = account->pw_name;
        if (uid >= 0 && static_cast<uid_t>(uid) == getuid())
          selected = identities.size() - 1;
      }
      // add() parses menu separators; escape untrusted identity labels.
      std::string escaped;
      for (char c : label) {
        if (strchr("/\\&_|", c))
          escaped += '\\';
        escaped += c;
      }
      identity.add(escaped.c_str());
    }
    if (identities.empty()) {
      finish(false, "No authentication identities supplied");
      return;
    }
    identity.value(selected);
    if (cancel_token) {
      cancellable = G_CANCELLABLE(g_object_ref(cancel_token));
      cancel_handler = g_cancellable_connect(
          cancellable,
          reinterpret_cast<GCallback>(+[](GCancellable *, gpointer p) {
            // Always defer: disconnecting the cancellable in its own callback
            // deadlocks.
            auto ui = static_cast<AgentUI *>(p);
            auto work = new std::pair<AgentUI *, unsigned>(ui, ui->generation);
            g_idle_add(
                +[](gpointer data) -> gboolean {
                  auto item =
                      static_cast<std::pair<AgentUI *, unsigned> *>(data);
                  if (item->first->task &&
                      item->first->generation == item->second)
                    item->first->abort();
                  delete item;
                  return G_SOURCE_REMOVE;
                },
                work);
          }),
          this, nullptr);
    }
    dx::style(&window);
    window.show();
    start();
  }
};
static AgentUI *ui;
typedef struct {
  PolkitAgentListener parent;
} DomeAgent;
typedef struct {
  PolkitAgentListenerClass parent;
} DomeAgentClass;
G_DEFINE_TYPE(DomeAgent, dome_agent, POLKIT_AGENT_TYPE_LISTENER)
static void dome_agent_init(DomeAgent *) {}
static void dome_agent_class_init(DomeAgentClass *klass) {
  auto parent = POLKIT_AGENT_LISTENER_CLASS(klass);
  parent->initiate_authentication =
      [](PolkitAgentListener *listener, const gchar *id, const gchar *message,
         const gchar *, PolkitDetails *, const gchar *cookie, GList *identities,
         GCancellable *cancel, GAsyncReadyCallback callback, gpointer data) {
        ui->begin(G_OBJECT(listener), id, message, cookie, identities, cancel,
                  callback, data);
      };
  parent->initiate_authentication_finish = [](PolkitAgentListener *,
                                              GAsyncResult *result,
                                              GError **error) -> gboolean {
    return g_task_propagate_boolean(G_TASK(result), error);
  };
}
static bool running = true;
int main(int argc, char **argv) {
  if (argc > 1 && !strcmp(argv[1], "--help")) {
    puts("dome-polkit-agent — native FLTK Polkit authentication agent");
    return 0;
  }
  struct rlimit limit{0, 0};
  setrlimit(RLIMIT_CORE, &limit);
  prctl(PR_SET_DUMPABLE, 0);
  dx::init();
  AgentUI dialog;
  ui = &dialog;
  if (argc > 1 && !strcmp(argv[1], "--preview")) {
    dialog.message.copy_label(
        "Authentication dialog preview — no authentication is performed.");
    dialog.action.copy_label("org.domed.preview");
    dialog.identity.add("Preview user");
    dialog.identity.value(0);
    dialog.cancel.callback([](Fl_Widget *, void *) { running = false; });
    dialog.window.callback([](Fl_Widget *, void *) { running = false; });
    dialog.submit.callback([](Fl_Widget *, void *) {
      ui->wipe();
      running = false;
    });
    dialog.window.show();
    while (running)
      Fl::wait(.05);
    dialog.wipe();
    return 0;
  }
  auto listener =
      POLKIT_AGENT_LISTENER(g_object_new(dome_agent_get_type(), nullptr));
  GError *error = nullptr;
  auto session_id = login_session::id();
  auto subject =
      !session_id.empty()
          ? polkit_unix_session_new(session_id.c_str())
          : polkit_unix_session_new_for_process_sync(getpid(), nullptr, &error);
  if (!subject) {
    fprintf(stderr, "Polkit session: %s\n",
            error ? error->message : "No graphical session found");
    g_clear_error(&error);
    return 1;
  }
  auto registration = polkit_agent_listener_register(
      listener, POLKIT_AGENT_REGISTER_FLAGS_NONE, subject,
      "/org/domed/PolicyKit1/AuthenticationAgent", nullptr, &error);
  g_object_unref(subject);
  if (!registration) {
    fprintf(stderr, "Polkit registration: %s\n", error->message);
    g_error_free(error);
    return 1;
  }
  signal(SIGTERM, [](int) { running = false; });
  signal(SIGINT, [](int) { running = false; });
  puts("Registered DomeD FLTK Polkit agent");
  fflush(stdout);
  while (running) {
    while (g_main_context_iteration(nullptr, FALSE)) {
    }
    Fl::wait(.02);
    if (!Fl::first_window())
      g_usleep(20000);
  }
  dialog.abort();
  polkit_agent_listener_unregister(registration);
  g_object_unref(listener);
  return 0;
}
