// SPDX-License-Identifier: MIT
#pragma once
#include "bdf_font.hpp"
#include <QtGui/QFontDatabase>
#include <QtCore/QJsonArray>
#include <QtCore/QJsonObject>
#include <QtCore/QJsonDocument>
#include <QtCore/QByteArray>
#include <set>
#include <unistd.h>
static QJsonArray bitmapFonts;
static QList<QByteArray> extraBitmapFonts;
extern "C" void dome_qt_queue_bitmap_font(const unsigned char *data, int length) {
    if (data && length > 0) extraBitmapFonts.append(QByteArray((const char *)data,length));
}
extern "C" const char *dome_qt_font_catalog(const unsigned char *data, int length) {
    app(); static QByteArray json; static std::set<std::string> registered;
    dome_qt_queue_bitmap_font(data,length);
    for (const auto &bytes : extraBitmapFonts) {
        if (!bytes.startsWith("STARTFONT ")) continue;
        unsigned long long hash=14695981039346656037ull;
        for(unsigned char c:bytes){hash^=c;hash*=1099511628211ull;}
        auto directory=std::filesystem::temp_directory_path()/("dome-bdf-"+std::to_string(getuid()));
        std::filesystem::create_directories(directory);
        auto path=directory/(std::to_string(hash)+".bdf");
        if(!std::filesystem::is_regular_file(path)){std::ofstream out(path,std::ios::binary);out.write(bytes.data(),bytes.size());}
        bdf_fonts::register_font(path.string());
    }
    extraBitmapFonts.clear();
    auto faces=bdf_fonts::discover();
    bitmapFonts={}; std::map<std::string,int> indices;
    for(const auto &face:faces){
        if(!face.text || face.pixels>64)continue;
        auto found=indices.find(face.id());
        if(found==indices.end()){
            indices[face.id()]=bitmapFonts.size();
            bitmapFonts.append(QJsonObject{{"id",QString::fromStdString(face.id())},{"family",QString::fromStdString(face.family)},
                {"style",QString::fromStdString(face.style)}, {"file",QString::fromStdString(face.file)},{"sizes",QJsonArray{face.pixels}}});
        }else{
            auto entry=bitmapFonts[found->second].toObject();auto sizes=entry["sizes"].toArray();
            if(!sizes.contains(face.pixels))sizes.append(face.pixels);
            std::vector<int> ordered;for(auto size:sizes)ordered.push_back(size.toInt());
            std::sort(ordered.begin(),ordered.end());sizes={};for(int size:ordered)sizes.append(size);
            entry["sizes"]=sizes;bitmapFonts[found->second]=entry;
        }
    }
    json=QJsonDocument(bitmapFonts).toJson(QJsonDocument::Compact);return json.constData();
}
static QFont interfaceFont(const QJsonObject &appearance) {
    auto id=appearance["font"].toString("Unifont / Sans-Serif");
    auto split=id.indexOf(" / "); auto family=id.left(split).toStdString(),style=id.mid(split+3).toStdString();
    int pixels=appearance["font_size"].toInt(16),scale=appearance["font_scale"].toInt(1);
    bdf_fonts::Face selected;
    try{selected=bdf_fonts::resolve(family,style,pixels,scale);}
    catch(const std::exception &){selected=bdf_fonts::resolve("Unifont","Sans-Serif",16,scale);}
    selected=bdf_fonts::qt_alias(selected);
    static std::map<std::string,QString> families;
    if(!families.count(selected.file)){
        int loaded=QFontDatabase::addApplicationFont(QString::fromStdString(selected.file));
        auto names=QFontDatabase::applicationFontFamilies(loaded);
        if(names.isEmpty())throw std::runtime_error("Qt cannot load BDF");
        families[selected.file]=names.first();
    }
    QFont font(families[selected.file]);font.setStyleName(QString::fromStdString(selected.style));font.setPixelSize(selected.pixels);
    font.setStyleStrategy(QFont::StyleStrategy(QFont::PreferBitmap|QFont::NoAntialias|QFont::NoFontMerging));
    return font;
}
