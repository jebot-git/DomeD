#!/usr/bin/env python3
"""Real Wayland popup/dialog fixture; driven only by the isolated integration test."""
import json
from pathlib import Path
import sys
from PySide6.QtCore import QPoint, Qt
from PySide6.QtGui import QColor, QPainter
from PySide6.QtWidgets import QApplication, QWidget

log = Path(sys.argv[1]).open("w", buffering=1)
def record(kind, **data):
    log.write(json.dumps({"kind": kind, **data}) + "\n")

app = QApplication([])
app.setApplicationName("dome-popup-fixture")
app.setDesktopFileName("dome-popup-fixture")
windows = []

class Panel(QWidget):
    def __init__(self, name, parent=None, dialog=False):
        flags = Qt.WindowType.FramelessWindowHint
        flags |= Qt.WindowType.Window if parent is None else Qt.WindowType.Dialog if dialog else Qt.WindowType.Popup
        super().__init__(parent, flags)
        self.name = name
        self.long_context = False
        self.setWindowTitle(name)
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        self.resize(720, 460) if parent is None else self.resize(240, 180)
        if parent:
            self.move(parent.mapToGlobal(QPoint(200, 100)))
        windows.append(self)
    def paintEvent(self, event):
        painter = QPainter(self)
        painter.fillRect(self.rect(), QColor({"parent": "#bd3e4e", "popup": "#36a381", "nested": "#3264c8", "dialog": "#b18e34", "context": "#9050b0"}[self.name]))
        if self.name == "context" and self.height() > 200:
            for y in range(0, self.height(), 100):
                painter.fillRect(0, y, self.width(), 100, QColor(40 + (y // 100 % 4) * 40, 100, 180))
        painter.setPen(QColor("white"))
        painter.drawText(16, 24, self.name)
    def keyPressEvent(self, event):
        record("key", panel=self.name, key=event.key())
        if event.key() == Qt.Key.Key_P and self.name == "parent":
            Panel("popup", self).show()
        elif event.key() == Qt.Key.Key_N and self.name == "popup":
            Panel("nested", self).show()
        elif event.key() == Qt.Key.Key_D and self.name == "parent":
            Panel("dialog", self, dialog=True).show()
        elif event.key() == Qt.Key.Key_L and self.name == "parent":
            self.long_context = not self.long_context
        elif event.key() == Qt.Key.Key_Escape and self.name != "parent":
            self.close()
        else:
            super().keyPressEvent(event)
    def mousePressEvent(self, event):
        record("press", panel=self.name, x=event.position().x(), y=event.position().y())
        if self.name == "parent" and event.button() == Qt.MouseButton.RightButton:
            menu = Panel("context", self)
            menu.resize(240, 1800 if self.long_context else 180)
            menu.show()
    def mouseReleaseEvent(self, event):
        record("release", panel=self.name)
    def wheelEvent(self, event):
        record("wheel", panel=self.name)
    def closeEvent(self, event):
        record("close", panel=self.name)
        super().closeEvent(event)

Panel("parent").show()
raise SystemExit(app.exec())
