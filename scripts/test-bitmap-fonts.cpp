// Compile with Qt6Widgets, fontconfig and freetype2; run with the bundled PCF path.
#include "../native/qt_chrome.cpp"
#include <QtGui/QRawFont>
#include <iostream>
#include <stdexcept>
static void require(bool ok,const char *message){if(!ok)throw std::runtime_error(message);}
int main(int argc,char **argv) try {
    require(argc==3,"Usage: test-bitmap-fonts FONT.pcf OUTPUT.png");
    QFile file(QString::fromLocal8Bit(argv[1]));require(file.open(QIODevice::ReadOnly),"Cannot read font");auto data=file.readAll();
    auto directory=QFileInfo(file).absoluteDir();
    for(const auto &relative:QStringList{
        "cozette-v1.30.0/cozette.otb",
        "denkichip-v1.213/DenkiChipHangul.bdf",
        "galmuri-v2.40.4/Galmuri11Bitmap-Regular-2.40.4.ttf",
        "galmuri-v2.40.4/Galmuri14Bitmap-Regular-2.40.4.ttf",
        "galmuri-v2.40.4/Galmuri9Bitmap-Regular-2.40.4.ttf",
        "galmuri-v2.40.4/Galmuri7Bitmap-Regular-2.40.4.ttf"}) {
        QFile extra(directory.filePath(relative));require(extra.open(QIODevice::ReadOnly),"Cannot read bundled bitmap font");auto bytes=extra.readAll();
        dome_qt_queue_bitmap_font(reinterpret_cast<const unsigned char*>(bytes.constData()),bytes.size());
    }
    auto catalog=QJsonDocument::fromJson(dome_qt_font_catalog(reinterpret_cast<const unsigned char*>(data.constData()),data.size())).array();
    require(!catalog.isEmpty(),"Bundled Unicode bitmap font missing");
    auto first=catalog.first().toObject();
    require(first["id"].toString()=="GNU Unifont / Sans-Serif","Unexpected bundled font identity");
    require(first["sizes"].toArray().contains(16),"Native 16 px strike missing");
    int cjk=0;bool cozette=false;
    for(auto entry:catalog) {
        auto choice=entry.toObject();
        for(auto size:choice["sizes"].toArray()) {
            auto selected=interfaceFont(QJsonObject{{"font",choice["id"]},{"font_size",size}});
            auto raw=QRawFont::fromFont(selected);
            require(raw.isValid()&&raw.familyName()==choice["family"].toString(),"A bitmap choice was silently substituted");
            require(raw.fontTable("glyf").isEmpty()&&raw.fontTable("CFF ").isEmpty()&&raw.fontTable("CFF2").isEmpty(),"An outline face was selected instead of a bitmap");
            require(raw.supportsCharacter(uint('A'))&&(raw.supportsCharacter(uint(0xe9))||raw.supportsCharacter(uint(0x3042))||raw.supportsCharacter(uint(0xac00))),"A bitmap choice lacks Unicode text coverage");
            auto family=choice["family"].toString();
            if(family=="Cozette") {
                cozette=true;
                require(size.toInt()==13,"Cozette must use its native 13 px strike");
                for(auto cp:QString::fromUtf8("Cozette é α Ж ┌─┐│└┘").toUcs4())
                    require(raw.supportsCharacter(cp),"Cozette sample glyph missing");
            }
            if(family.contains("DenkiChip")||family.startsWith("Galmuri")) {
                ++cjk;
                for(auto cp:QString::fromUtf8("日本語 あいうえお カタカナ 한국어 한글 가나다").toUcs4())
                    require(raw.supportsCharacter(cp),"A bundled CJK face requires glyph substitution for sample text");
                if(family.startsWith("Galmuri"))
                    for(uint cp=0xac00;cp<=0xd7a3;++cp)require(raw.supportsCharacter(cp),"Galmuri must cover every modern Hangul syllable");
            }
        }
    }
    require(cozette,"Cozette bitmap option must be available");
    require(cjk==5,"DenkiChip and all four Galmuri bitmap options must be available");
    QImage samples(1100,360,QImage::Format_ARGB32_Premultiplied);samples.fill(QColor(31,37,45));
    {
        QPainter p(&samples);p.setPen(QColor(231,235,241));int y=12;
        for(auto entry:catalog) {
            auto choice=entry.toObject();auto family=choice["family"].toString();
            if(!family.contains("DenkiChip")&&!family.startsWith("Galmuri"))continue;
            p.setFont(interfaceFont(QJsonObject{}));p.drawText(QRect(16,y,320,48),Qt::AlignVCenter,family);
            p.setFont(interfaceFont(QJsonObject{{"font",choice["id"]},{"font_size",choice["sizes"].toArray().first()}}));
            p.drawText(QRect(350,y,730,48),Qt::AlignVCenter,QString::fromUtf8("日本語 あいうえお カタカナ 한국어 한글 가나다"));y+=64;
        }
    }
    require(samples.save(QString::fromLocal8Bit(argv[2])+"-cjk.png"),"Cannot save CJK samples");
    FT_Library library;require(!FT_Init_FreeType(&library),"Cannot initialize FreeType");
    auto query=FcNameParse(reinterpret_cast<const FcChar8*>("sans:scalable=true"));
    FcConfigSubstitute(nullptr,query,FcMatchPattern);FcDefaultSubstitute(query);FcResult result;
    auto match=FcFontMatch(nullptr,query,&result);FcChar8 *path=nullptr;
    require(match&&FcPatternGetString(match,FC_FILE,0,&path)==FcResultMatch,"No outline test font");
    FT_Face face;require(!FT_New_Face(library,reinterpret_cast<const char*>(path),0,&face),"Cannot open outline font");
    require(FT_IS_SCALABLE(face),"Test font must have outlines");
    int before=bitmapFonts.size();addBitmapFont(face,{},QString::fromUtf8(reinterpret_cast<const char*>(path)));
    require(bitmapFonts.size()==before,"Outline font must not enter the picker");
    FT_Done_Face(face);FT_Done_FreeType(library);FcPatternDestroy(match);FcPatternDestroy(query);
    QJsonObject appearance{{"font",first["id"]},{"font_size",16},{"opacity",248}};
    auto font=interfaceFont(appearance);auto raw=QRawFont::fromFont(font);
    require(font.pixelSize()==16&&!QFontDatabase::isSmoothlyScalable(font.family(),font.styleName()),"Selection must use a native bitmap strike");
    require(raw.familyName()==first["family"].toString(),"Selected bitmap font must not be substituted");
    for(auto cp:{0x00e9u,0x03b1u,0x0416u,0x306bu,0x65e5u})require(raw.supportsCharacter(cp),"Unicode glyph missing");
    appearance["font_size"]=19;require(interfaceFont(appearance).pixelSize()==16,"Unsupported sizes must resolve to a native strike");
    appearance["font_size"]=16;
    QJsonObject scene{{"width",1000},{"height",700},{"kind","shell"},{"appearance",appearance},{"appearance_open",true},{"keyboard_settings",QJsonValue::Null}};
    auto json=QJsonDocument(scene).toJson();QImage image(1000,700,QImage::Format_ARGB32_Premultiplied);
    require(dome_qt_paint(json.constData(),image.bits(),image.sizeInBytes())==1,"Appearance render failed");
    require(image.save(QString::fromLocal8Bit(argv[2])),"Cannot save preview");
    appearance["font"]="Cozette / Medium";appearance["font_size"]=13;
    scene["appearance"]=appearance;json=QJsonDocument(scene).toJson();
    require(dome_qt_paint(json.constData(),image.bits(),image.sizeInBytes())==1,"Cozette appearance render failed");
    require(image.save(QString::fromLocal8Bit(argv[2])+"-cozette.png"),"Cannot save Cozette preview");
    std::cout<<"PASS: Unicode bitmap catalog, outline rejection, native strikes, multilingual glyphs and appearance rendering\n";
} catch(const std::exception &error){std::cerr<<error.what()<<'\n';return 1;}
