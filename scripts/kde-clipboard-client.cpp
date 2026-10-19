// Test fixture for the clipboard API used by KDE Connect; no network or window.
#include <QGuiApplication>
#include <QTimer>
#include <QMimeData>
#include <QJsonDocument>
#include <QJsonObject>
#include <KSystemClipboard>
#include <cstdio>
int main(int argc,char **argv){
    QGuiApplication app(argc,argv);
    auto clipboard=KSystemClipboard::instance();
    QObject::connect(clipboard,&KSystemClipboard::changed,&app,[&](QClipboard::Mode mode){
        if(mode!=QClipboard::Clipboard)return;
        auto json=QJsonDocument(QJsonObject{{"text",clipboard->text(mode)}}).toJson(QJsonDocument::Compact);
        std::puts(json.constData());std::fflush(stdout);
    });
    QTimer::singleShot(500,&app,[&]{auto mime=new QMimeData;mime->setText("DomeD KDE background clipboard");clipboard->setMimeData(mime,QClipboard::Clipboard);});
    QTimer::singleShot(15000,&app,&QCoreApplication::quit);
    return app.exec();
}
