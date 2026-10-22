// Qt BDF catalog/strike regression. Compile with Qt6Widgets, Fontconfig and zlib.
#include "../native/qt_chrome.cpp"
#include <QtGui/QRawFont>
#include <iostream>
#include <stdexcept>
static void require(bool ok,const char *message){if(!ok)throw std::runtime_error(message);}
int main(int argc,char **argv) try {
    require(argc==3,"Usage: test-bitmap-fonts FONT_DIRECTORY OUTPUT.png");
    setenv("DE_XR_BDF_DIRECTORY",argv[1],1);
    auto catalog=QJsonDocument::fromJson(dome_qt_font_catalog(nullptr,0)).array();
    require(!catalog.isEmpty(),"Raw BDF catalog empty");
    int checked=0;
    for(auto item:catalog){
        auto entry=item.toObject();
        for(auto size:entry["sizes"].toArray())for(int scale:{1,2}){
            auto font=interfaceFont(QJsonObject{{"font",entry["id"]},{"font_size",size},{"font_scale",scale}});
            auto raw=QRawFont::fromFont(font);
            std::cerr<<entry["id"].toString().toStdString()<<" "<<size.toInt()<<"x"<<scale<<" -> "<<raw.familyName().toStdString()<<" "<<raw.styleName().toStdString()<<"\n";
            require(raw.isValid(),"Qt rejected a native BDF");
            require(raw.pixelSize()==size.toInt()*scale,"Qt silently scaled a strike");
            require(raw.fontTable("glyf").isEmpty()&&raw.fontTable("CFF ").isEmpty()&&raw.fontTable("CFF2").isEmpty(),"Outline font selected");
            require(raw.supportsCharacter(uint('A')),"Selected BDF lost its Unicode character map");
            ++checked;
        }
    }
    QImage image(1200,500,QImage::Format_ARGB32_Premultiplied);image.fill(QColor(31,37,45));
    QPainter painter(&image);painter.setPen(QColor(231,235,241));
    for(int scale:{1,2}){
        painter.setFont(interfaceFont(QJsonObject{{"font","Unifont / Sans-Serif"},{"font_size",16},{"font_scale",scale}}));
        painter.drawText(24,scale==1?60:180,QString::fromUtf8("BDF · Příliš žluťoučký kůň · 日本語 · 한국어 · العربية"));
    }
    painter.end();require(image.save(QString::fromLocal8Bit(argv[2])),"Save Qt sample");
    std::cout<<"PASS: "<<checked<<" Qt native BDF selections at both scales\n";
} catch(const std::exception &e){std::cerr<<e.what()<<'\n';return 1;}
