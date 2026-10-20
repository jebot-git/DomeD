// Disposable real Qt Wayland menu exporter for test-global-menu.py.
#include <QtWidgets/QApplication>
#include <QtWidgets/QMainWindow>
#include <QtWidgets/QMenuBar>
#include <QtWidgets/QMenu>
#include <QtWidgets/QLineEdit>
#include <QtCore/QFile>
#include <QtCore/QTimer>
int main(int argc,char **argv) {
    QApplication app(argc,argv);app.setApplicationName("Dome Menu Fixture");app.setDesktopFileName("dome-menu-fixture");
    QMainWindow window;QString name=argc>1?QString::fromLocal8Bit(argv[1]):"fixture";window.setWindowTitle(name);window.resize(640,420);
    auto *input=new QLineEdit(&window);input->setPlaceholderText("Keypad input");window.setCentralWidget(input);input->setFocus();
    QObject::connect(input,&QLineEdit::textEdited,[&](const QString &text){window.setWindowTitle(name+"-key-"+text);});
    if(name=="plain"){window.show();return app.exec();}
    int activations=0;
    auto *file=window.menuBar()->addMenu("&Fixture");
    auto *activate=file->addAction("&Activate fixture");activate->setShortcut(QKeySequence("Ctrl+Shift+F12"));
    QObject::connect(activate,&QAction::triggered,[&]{window.setWindowTitle(name+"-activated-"+QString::number(++activations));});
    auto *disabled=file->addAction("Disabled action");disabled->setEnabled(false);
    QObject::connect(disabled,&QAction::triggered,[&]{window.setWindowTitle("disabled-activated");});
    file->addSeparator();auto *submenu=file->addMenu("&Nested");
    auto *check=submenu->addAction("Checkable action");check->setCheckable(true);
    QObject::connect(check,&QAction::triggered,[&]{window.setWindowTitle(name+"-nested-activated");});
    auto *edit=window.menuBar()->addMenu("&Changes");
    edit->addAction("Change label",[&]{activate->setText("Updated action");});
    window.show();return app.exec();
}
