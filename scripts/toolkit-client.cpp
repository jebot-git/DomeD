// SPDX-License-Identifier: MIT
// Native Qt fixture: record real toolkit font/palette changes, without custom styling.
#include <QApplication>
#include <QWidget>
#include <QVBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QFontInfo>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTimer>
int main(int argc,char **argv) {
    QApplication app(argc,argv);app.setDesktopFileName("org.domed.ToolkitQt");
    if(argc!=2)return 2;
    QString output=QString::fromLocal8Bit(argv[1]);
    QWidget window;window.setWindowTitle("Qt appearance fixture");
    auto layout=new QVBoxLayout(&window);layout->addWidget(new QLabel("Session appearance · 日本語 · Žluťoučký"));
    layout->addWidget(new QLineEdit("Toolkit text field"));layout->addWidget(new QPushButton("Toolkit button"));
    window.resize(480,240);window.show();
    QTimer timer;QObject::connect(&timer,&QTimer::timeout,[&]{
        QFontInfo font(app.font());auto palette=app.palette();
        QJsonObject record{{"family",font.family()},{"pixels",font.pixelSize()},
            {"window",palette.color(QPalette::Window).name()},{"text",palette.color(QPalette::WindowText).name()},
            {"accent",palette.color(QPalette::Highlight).name()}};
        QFile file(output+".tmp");if(file.open(QIODevice::WriteOnly)){file.write(QJsonDocument(record).toJson());file.close();QFile::remove(output);file.rename(output);}
    });timer.start(100);return app.exec();
}
