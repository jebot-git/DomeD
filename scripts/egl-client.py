#!/usr/bin/env python3
"""Native Wayland EGL fixture: requires a working compositor DMA-BUF path."""
from PySide6.QtGui import QGuiApplication, QSurfaceFormat
from PySide6.QtOpenGL import QOpenGLWindow

format = QSurfaceFormat()
format.setRenderableType(QSurfaceFormat.RenderableType.OpenGLES)
format.setVersion(2, 0)
QSurfaceFormat.setDefaultFormat(format)
app = QGuiApplication([])
app.setApplicationName("dome-egl-fixture")
app.setDesktopFileName("dome-egl-fixture")


class Window(QOpenGLWindow):
    def paintGL(self):
        functions = self.context().functions()
        functions.glClearColor(0.2, 0.3, 0.9, 1.0)
        functions.glClear(0x4000)


window = Window()
window.setTitle("Dome EGL fixture")
window.resize(640, 480)
window.show()
raise SystemExit(app.exec())
