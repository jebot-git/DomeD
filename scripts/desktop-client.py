#!/usr/bin/env python3
"""Disposable native Wayland client for desktop input/rendering integration tests."""
import argparse,json,time
from pathlib import Path
from PySide6.QtCore import Qt,QTimer
from PySide6.QtGui import QColor,QPainter
from PySide6.QtWidgets import QApplication,QWidget
p=argparse.ArgumentParser();p.add_argument('--name',required=True);p.add_argument('--app-id');p.add_argument('--color',default='#bd3e4e');p.add_argument('--journal',required=True);a=p.parse_args()
log=Path(a.journal).open('a',buffering=1)
def record(kind,**fields):log.write(json.dumps({'kind':kind,'time':time.monotonic(),**fields})+'\n')
app=QApplication([]);app.setApplicationName(a.name);app.setDesktopFileName(a.app_id or a.name)
class Client(QWidget):
    def __init__(self):
        super().__init__(None,Qt.WindowType.Window|Qt.WindowType.FramelessWindowHint)
        self.setWindowTitle(a.name);self.setFocusPolicy(Qt.FocusPolicy.StrongFocus);self.resize(720,460);self.setMinimumSize(160,100)
    def paintEvent(self,event):
        painter=QPainter(self);painter.fillRect(self.rect(),QColor(a.color));painter.setPen(QColor('white'));painter.drawText(18,22,a.name+' | drag here');painter.drawText(18,60,'Native Wayland test client');painter.drawText(self.width()-100,self.height()-14,'resize corner')
    def resizeEvent(self,event):record('resize',w=event.size().width(),h=event.size().height())
    def focusInEvent(self,event):record('focus_in')
    def focusOutEvent(self,event):record('focus_out')
    def keyPressEvent(self,event):
        record('key_press',text=event.text(),key=event.key(),modifiers=event.modifiers().value)
        if event.modifiers()&Qt.KeyboardModifier.ControlModifier:
            if event.key()==Qt.Key.Key_C:app.clipboard().setText(a.name+'-clipboard');record('clipboard_set')
            elif event.key()==Qt.Key.Key_V:record('clipboard_get',text=app.clipboard().text())
    def keyReleaseEvent(self,event):record('key_release',text=event.text(),key=event.key())
    def mousePressEvent(self,event):
        x,y=event.position().x(),event.position().y();record('press',x=x,y=y,button=event.button().value)
        if y<28:record('move_request',accepted=self.windowHandle().startSystemMove())
        elif x>self.width()-24 and y>self.height()-24:record('resize_request',accepted=self.windowHandle().startSystemResize(Qt.Edge.RightEdge|Qt.Edge.BottomEdge))
    def mouseReleaseEvent(self,event):record('release',x=event.position().x(),y=event.position().y(),button=event.button().value)
    def wheelEvent(self,event):record('wheel',y=event.angleDelta().y())
    def closeEvent(self,event):record('close');super().closeEvent(event)
window=Client();window.show();timer=QTimer();timer.timeout.connect(window.update);timer.start(250)
raise SystemExit(app.exec())
