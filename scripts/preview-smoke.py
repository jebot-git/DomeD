#!/usr/bin/env python3
"""Offscreen app test; verifies both modes, workspace manager and quick app search.
Requires a running dome-preview. Writes PNGs in artifacts/; exits on JS errors.
Run: uv run --no-project --with PySide6 python scripts/preview-smoke.py
"""
import os,sys
from pathlib import Path
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
os.environ.setdefault('QTWEBENGINE_CHROMIUM_FLAGS','--disable-gpu --disable-dev-shm-usage')
from PySide6.QtWidgets import QApplication
from PySide6.QtWebEngineWidgets import QWebEngineView
from PySide6.QtWebEngineCore import QWebEnginePage
from PySide6.QtCore import QTimer,QUrl
errors=[]
class Page(QWebEnginePage):
    def javaScriptConsoleMessage(self,level,message,line,source):
        if level==QWebEnginePage.JavaScriptConsoleMessageLevel.ErrorMessageLevel:errors.append(f'{source}:{line}: {message}')
app=QApplication([]);view=QWebEngineView();page=Page(view);view.setPage(page);view.resize(1500,1050);view.show()
root=Path(__file__).resolve().parents[1];(root/'artifacts').mkdir(exist_ok=True)
steps=[]
def js(code):page.runJavaScript(code)
def check(code):
    def result(value):
        if not value:errors.append('Failed: '+code)
    page.runJavaScript(code,result)
def capture(name):view.grab().save(str(root/'artifacts'/name))
def done():
    print('Preview interactions:', 'passed' if not errors else errors)
    app.exit(1 if errors else 0)
def start():
    check("Boolean($('tasks').children.length && $('radius_m').value && $('scene').width)")
    check("!$('upper_inset_deg') && !$('lower_inset_deg') && !$('upper_bar_max_width_deg') && !$('taskbar_max_width_deg') && snapshot.shell_bars.length===2 && snapshot.shell_bars.every(bar=>bar.dome)")
    capture('dome-preview.png')
    js("$('toggle-view').click()")
    steps.extend([
      lambda:check("view==='desktop' && !document.querySelector('#projection') && snapshot.scene.config.projection==='room_dome'"),
      lambda:capture('dome-desktop-toggle.png'),
      lambda:js("$('desktops').click();$('workspace-name').value='UI smoke';$('new-workspace').requestSubmit()"),
      lambda:check("$('manager').open && snapshot.scene.workspaces.some(w=>w.name==='UI smoke')"),
      lambda:js("const cards=[...document.querySelectorAll('.workspace-card')];cards.at(-1).children[1].click();cards.at(-1).querySelector('input').value='Renamed';cards.at(-1).querySelector('button').click()"),
      lambda:check("snapshot.scene.workspaces.some(w=>w.name==='Renamed')"),
      lambda:js("document.querySelectorAll('.workspace-card').item(snapshot.scene.workspaces.length-1).lastElementChild.click()"),
      lambda:check("!snapshot.scene.workspaces.some(w=>w.name==='Renamed')"),
      lambda:js("$('manager').close();$('launch').click()"),
      lambda:check("$('launcher').open && document.activeElement===$('search') && apps.length>0"),
      lambda:js("$('search').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}))"),
      lambda:check("launchIndex===Math.min(1,matches().length-1) && $('app-results').children[launchIndex].classList.contains('selected')"),
      lambda:js("$('search').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowUp',bubbles:true}))"),
      lambda:check("launchIndex===0"),
      lambda:js("$('search').value='no-such-app-928354';$('search').dispatchEvent(new Event('input'))"),
      lambda:check("$('app-results').textContent==='No matching applications'"),
      lambda:js("$('launcher').close();$('desktop').click()"),
      lambda:capture('dome-desktop.png'),
      lambda:js("$('xr').click();$('unfold').click()"),
      lambda:capture('dome-unfolded.png'),
      lambda:js("$('unfold').click()"),
      done])
    QTimer.singleShot(700,advance)
def advance():
    if steps:
        steps.pop(0)();QTimer.singleShot(700,advance)
QTimer.singleShot(3000,start)
view.load(QUrl(os.environ.get('DOME_PREVIEW_URL','http://127.0.0.1:49640')))
QTimer.singleShot(25000,lambda:app.exit(2))
sys.exit(app.exec())
