#!/usr/bin/python3
"""GTK3 application-menu and popup fixture for isolated compositor tests."""
import json, os, sys
import gi
gtk4 = '--gtk4' in sys.argv
gi.require_version('Gtk', '4.0' if gtk4 else '3.0')
from gi.repository import Gtk, Gio, GLib
legacy = '--legacy' in sys.argv
# Each synthetic application needs a distinct Wayland identity despite sharing Python.
GLib.set_prgname('org.domed.GtkLegacyFixture' if legacy else 'org.domed.Gtk4MenuFixture' if gtk4 else 'org.domed.GtkMenuFixture')

def record(**value): print(json.dumps(value), flush=True)

def populate(window):
    window.set_title('GTK legacy fixture' if legacy else 'GTK menu fixture')
    window.set_default_size(720,460);window.set_decorated(False)
    box=Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
    window.add(box)
    if legacy:
        bar=Gtk.MenuBar();box.pack_start(bar,False,False,0)
        file=Gtk.MenuItem.new_with_label('Legacy');bar.append(file)
        menu=Gtk.Menu();file.set_submenu(menu)
        action=Gtk.MenuItem.new_with_label('Activate legacy');menu.append(action)
        action.connect('activate',lambda *_:window.set_title('GTK legacy activated'))
        disabled=Gtk.MenuItem.new_with_label('Disabled');disabled.set_sensitive(False);menu.append(disabled)
    fixed=Gtk.Fixed();box.pack_start(fixed,True,True,0)
    button=Gtk.MenuButton(label='Popup test');button.set_size_request(160,40);fixed.put(button,50,60)
    popup=Gtk.Menu();button.set_popup(popup)
    item=Gtk.MenuItem.new_with_label('Popup action');popup.append(item)
    item.connect('activate',lambda *_:window.set_title('GTK popup activated'))
    nested=Gtk.MenuItem.new_with_label('Nested');popup.append(nested)
    submenu=Gtk.Menu();nested.set_submenu(submenu)
    item=Gtk.MenuItem.new_with_label('Nested action');submenu.append(item)
    item.connect('activate',lambda *_:window.set_title('GTK nested activated'))
    popup.show_all();window.show_all()
    settings=Gtk.Settings.get_default()
    record(menubar=settings.get_property('gtk-shell-shows-menubar'), appmenu=settings.get_property('gtk-shell-shows-app-menu'),wayland=os.environ.get('WAYLAND_DISPLAY'),modules=os.environ.get('GTK_MODULES',''))
    if legacy:window.connect('destroy',Gtk.main_quit)
    return window

if gtk4:
    app=Gtk.Application(application_id='org.domed.Gtk4MenuFixture')
    def activate4(application):
        window=Gtk.ApplicationWindow(application=application);window.set_title('GTK4 menu fixture');window.set_default_size(720,460);window.set_decorated(False)
        action=Gio.SimpleAction.new('activate',None);action.connect('activate',lambda *_:window.set_title('GTK4 activated'));application.add_action(action)
        file=Gio.Menu();file.append('Activate GTK4','app.activate')
        menu=Gio.Menu();menu.append_submenu('File',file);application.set_menubar(menu);window.set_show_menubar(True)
        fixed=Gtk.Fixed();window.set_child(fixed)
        button=Gtk.MenuButton(label='Popup test');button.set_size_request(160,40);button.set_menu_model(file);fixed.put(button,50,60)
        window.present()
    app.connect('activate',activate4)
    raise SystemExit(app.run([sys.argv[0]]))
elif legacy:
    populate(Gtk.Window());Gtk.main()
else:
    app=Gtk.Application(application_id='org.domed.GtkMenuFixture')
    def activate(application):
        existing=next(iter(application.get_windows()),None)
        if existing:
            existing.present();return
        window=Gtk.ApplicationWindow(application=application)
        for name in ['activate','disabled','nested']:
            action=Gio.SimpleAction.new(name,None)
            action.set_enabled(name!='disabled')
            action.connect('activate',lambda a,p:window.set_title('GTK '+a.get_name()+' activated'))
            application.add_action(action)
        toggle=Gio.SimpleAction.new_stateful('toggle',None,GLib.Variant('b',False))
        toggle.connect('activate',lambda a,p:(a.set_state(GLib.Variant('b',not a.get_state().get_boolean())),window.set_title('GTK toggle '+str(a.get_state().get_boolean()))))
        application.add_action(toggle)
        menu=Gio.Menu();file=Gio.Menu();file.append('Activate GTK','app.activate');file.append('Disabled','app.disabled');file.append('Toggle','app.toggle')
        nested=Gio.Menu();nested.append('Nested action','app.nested');file.append_submenu('Nested',nested)
        menu.append_submenu('File',file);application.set_menubar(menu)
        appmenu=Gio.Menu();appmenu.append('About fixture','app.activate');application.set_app_menu(appmenu)
        populate(window)
    app.connect('activate',activate)
    raise SystemExit(app.run([sys.argv[0]]))
