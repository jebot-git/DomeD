// SPDX-License-Identifier: MIT
#pragma once
#include <algorithm>
#include <QtCore/QFile>
#include <QtGui/QFontDatabase>
#include <QtCore/QDirIterator>
#include <QtCore/QStandardPaths>
#include <QtCore/QSet>
#include <fontconfig/fontconfig.h>
#include <ft2build.h>
#include FT_FREETYPE_H

// Keep the actual fixed strikes. Neither outline fonts nor symbol-only bitmaps
// are useful choices for the global interface font.
static QJsonArray bitmapFonts;
static QList<QByteArray> extraBitmapFonts;
extern "C" void dome_qt_queue_bitmap_font(const unsigned char *data, int length) {
    extraBitmapFonts.append(QByteArray(reinterpret_cast<const char*>(data),length));
}
static void addBitmapFont(FT_Face face, const QByteArray &data, const QString &path) {
    if (FT_IS_SCALABLE(face) || !face->num_fixed_sizes ||
        FT_Select_Charmap(face, FT_ENCODING_UNICODE) ||
        !FT_Get_Char_Index(face, 'A') ||
        !(FT_Get_Char_Index(face, 0x00e9) || FT_Get_Char_Index(face, 0x3042) || FT_Get_Char_Index(face, 0xac00))) return;
    QJsonArray sizes;
    QList<int> strikes;
    for (int i=0; i<face->num_fixed_sizes; ++i) {
        int size=qRound(face->available_sizes[i].y_ppem/64.);
        // Keep desktop text within the existing controls' usable range.
        if(size>=12 && size<=24 && !strikes.contains(size)) strikes.append(size);
    }
    std::sort(strikes.begin(),strikes.end());
    for(int size:strikes) sizes.append(size);
    if(sizes.isEmpty())return;
    int id = data.isEmpty() ? QFontDatabase::addApplicationFont(path)
                           : QFontDatabase::addApplicationFontFromData(data);
    if (id < 0) return;
    for(const auto &family:QFontDatabase::applicationFontFamilies(id)) {
        QString style=QString::fromUtf8(face->style_name ? face->style_name : "Regular");
        QString key=family+" / "+style;
        bool found=false;
        for(int index=0;index<bitmapFonts.size();++index) {
            auto font=bitmapFonts[index].toObject();
            if(font["id"].toString()!=key)continue;
            auto existing=font["sizes"].toArray();
            for(auto size:sizes)if(!existing.contains(size))existing.append(size);
            QList<int> merged;for(auto size:existing)merged.append(size.toInt());
            std::sort(merged.begin(),merged.end());existing={};for(auto size:merged)existing.append(size);
            font["sizes"]=existing;bitmapFonts[index]=font;found=true;break;
        }
        if(!found) bitmapFonts.append(QJsonObject{{"id",key},{"family",family},{"style",style},{"sizes",sizes}});
    }
}
extern "C" const char *dome_qt_font_catalog(const unsigned char *bundled, int length) {
    app();
    static QByteArray json;
    if(!json.isEmpty())return json.constData();
    FT_Library library;
    if(FT_Init_FreeType(&library)) { json="[]";return json.constData(); }
    FT_Face face;
    if(!FT_New_Memory_Face(library,bundled,length,0,&face)) {
        addBitmapFont(face,QByteArray(reinterpret_cast<const char*>(bundled),length),{});
        FT_Done_Face(face);
    }
    for(const auto &data:extraBitmapFonts) {
        if(!FT_New_Memory_Face(library,reinterpret_cast<const FT_Byte*>(data.constData()),data.size(),0,&face)) {
            addBitmapFont(face,data,{});FT_Done_Face(face);
        }
    }
    extraBitmapFonts.clear();
    QSet<QString> paths;
    auto query=FcPatternCreate();
    FcPatternAddBool(query,FC_SCALABLE,FcFalse);
    auto objects=FcObjectSetBuild(FC_FILE,nullptr);
    auto fonts=FcFontList(nullptr,query,objects);
    if(fonts) {
        for(int i=0;i<fonts->nfont;++i) {
            FcChar8 *file=nullptr;
            if(FcPatternGetString(fonts->fonts[i],FC_FILE,0,&file)==FcResultMatch)
                paths.insert(QString::fromUtf8(reinterpret_cast<const char*>(file)));
        }
        FcFontSetDestroy(fonts);
    }
    FcObjectSetDestroy(objects);FcPatternDestroy(query);
    // Explicit application loading also supports desktops which disable bitmap
    // fonts in their system Fontconfig policy, without changing that policy.
    QStringList directories={"/usr/share/fonts","/usr/local/share/fonts",QDir::homePath()+"/.fonts"};
    for(const auto &directory:QStandardPaths::standardLocations(QStandardPaths::GenericDataLocation))
        directories.append(directory+"/fonts");
    directories.removeDuplicates();
    for(const auto &directory:directories) {
        QDirIterator files(directory,{"*.pcf","*.pcf.gz","*.bdf","*.bdf.gz","*.otb","*.fon","*.fnt"},QDir::Files,QDirIterator::Subdirectories);
        while(files.hasNext())paths.insert(files.next());
    }
    auto ordered=paths.values();std::sort(ordered.begin(),ordered.end());
    for(const auto &path:ordered) {
        auto file=QFile::encodeName(path);
        if(!FT_New_Face(library,file.constData(),0,&face)) {
            addBitmapFont(face,{},path);FT_Done_Face(face);
        }
    }
    FT_Done_FreeType(library);
    json=QJsonDocument(bitmapFonts).toJson(QJsonDocument::Compact);
    return json.constData();
}
static QFont interfaceFont(const QJsonObject &appearance) {
    QString selected=appearance["font"].toString("Unifont / Sans-Serif");
    QJsonObject choice;
    for(const auto &font:bitmapFonts)if(font.toObject()["id"].toString()==selected){choice=font.toObject();break;}
    if(choice.isEmpty() && !bitmapFonts.isEmpty())choice=bitmapFonts.first().toObject();
    if(choice.isEmpty())return app()->font();
    auto sizes=choice["sizes"].toArray();int size=appearance["font_size"].toInt(16);
    if(!sizes.contains(size))size=sizes.first().toInt();
    QFont font(choice["family"].toString());font.setStyleName(choice["style"].toString());font.setPixelSize(size);
    font.setStyleStrategy(QFont::StyleStrategy(QFont::PreferBitmap|QFont::NoAntialias));
    return font;
}
