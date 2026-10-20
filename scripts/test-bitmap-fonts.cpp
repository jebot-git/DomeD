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
    QDirIterator bundled(directory.absolutePath(), {"*.pcf", "*.bdf", "*.otb", "*.ttf"}, QDir::Files, QDirIterator::Subdirectories);
    while(bundled.hasNext()) {
        auto path=bundled.next();
        if(path==file.fileName() || QFileInfo(path).fileName()=="upstream.bdf")continue;
        QFile extra(path);require(extra.open(QIODevice::ReadOnly),"Cannot read bundled bitmap font");auto bytes=extra.readAll();
        dome_qt_queue_bitmap_font(reinterpret_cast<const unsigned char*>(bytes.constData()),bytes.size());
    }
    auto catalog=QJsonDocument::fromJson(dome_qt_font_catalog(reinterpret_cast<const unsigned char*>(data.constData()),data.size())).array();
    QFile catalogFile(QString::fromLocal8Bit(argv[2])+"-catalog.json");if(catalogFile.open(QIODevice::WriteOnly))catalogFile.write(QJsonDocument(catalog).toJson());
    require(!catalog.isEmpty(),"Bundled Unicode bitmap font missing");
    auto first=catalog.first().toObject();
    require(first["id"].toString()=="Unifont / Sans-Serif","Unexpected bundled font identity");
    require(first["sizes"].toArray().contains(16),"Native 16 px strike missing");
    int cjk=0;bool cozette=false;QSet<QString> bundledFamilies;
    for(auto entry:catalog) {
        auto choice=entry.toObject();
        bundledFamilies.insert(choice["family"].toString());
        for(auto size:choice["sizes"].toArray()) {
            require(size.toInt()>=12 && size.toInt()<=24,"Only native strikes of at least 12px may enter the picker");
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
            if(family=="DungGeunMo") {
                require(size.toInt()==16,"DungGeunMo must use the intended 16px strike");
                for(uint cp=0xac00;cp<=0xd7a3;++cp)require(raw.supportsCharacter(cp),"DungGeunMo must cover every modern Hangul syllable");
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
    require(cjk==4,"DenkiChip and the three Galmuri faces of at least 12px must be available");
    for(const auto &family:QStringList{"DungGeunMo", "kirsch", "Spleen", "TamzenForPowerline", "Ttyp0", "Fusion Pixel 12px Mono ja", "Fusion Pixel 12px Mono ko", "Fusion Pixel 12px Mono zh_Hans", "Fusion Pixel 12px Mono zh_Hant"})
        require(bundledFamilies.contains(family),"New bundled bitmap family missing");
    require(!bundledFamilies.contains("Galmuri7Bitmap"),"Sub-12px Galmuri must not enter the picker");
    QImage samples(1400,catalog.size()*96+24,QImage::Format_ARGB32_Premultiplied);samples.fill(QColor(31,37,45));
    {
        QPainter p(&samples);p.setPen(QColor(231,235,241));int y=12;
        for(auto entry:catalog) {
            auto choice=entry.toObject();auto family=choice["family"].toString();
            p.setFont(interfaceFont(QJsonObject{}));p.drawText(QRect(16,y,410,80),Qt::AlignVCenter,family);
            p.setFont(interfaceFont(QJsonObject{{"font",choice["id"]},{"font_size",choice["sizes"].toArray().first()}}));
            QString sample="Příliš žluťoučký kůň · ÄÖÜ äöü ß · Русский Українська Српски";
            if(family=="DungGeunMo"||family.contains("DenkiChip")||family.startsWith("Galmuri")||family.startsWith("Fusion"))sample="日本語 あいうえお カタカナ 한국어 한글 简体中文 繁體中文";
            if(family=="Ttyp0")sample+="\nภาษาไทย สวัสดี กำ กี่ กุ กู";
            if(family=="Unifont")sample+="\nالعربية مرحبا بالعالم لا لأ لإ لآ · ภาษาไทย สวัสดี";
            if(family=="Cozette"||family=="kirsch"||family=="Spleen"||family=="TamzenForPowerline"||family=="Ttyp0")sample+="\n               ";
            p.drawText(QRect(440,y,940,80),Qt::AlignVCenter,sample);y+=96;
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
    FT_Done_Face(face);
    const QByteArray smallFont=R"BDF(STARTFONT 2.1
FONT -DomeD-DomeSmallTest-Medium-R-Normal--10-100-72-72-C-60-ISO10646-1
SIZE 10 72 72
FONTBOUNDINGBOX 6 10 0 -2
STARTPROPERTIES 5
FONT_ASCENT 8
FONT_DESCENT 2
FAMILY_NAME "DomeSmallTest"
CHARSET_REGISTRY "ISO10646"
CHARSET_ENCODING "1"
ENDPROPERTIES
CHARS 2
STARTCHAR A
ENCODING 65
SWIDTH 600 0
DWIDTH 6 0
BBX 6 1 0 0
BITMAP
FC
ENDCHAR
STARTCHAR eacute
ENCODING 233
SWIDTH 600 0
DWIDTH 6 0
BBX 6 1 0 0
BITMAP
FC
ENDCHAR
ENDFONT
)BDF";
    require(!FT_New_Memory_Face(library,reinterpret_cast<const FT_Byte*>(smallFont.constData()),smallFont.size(),0,&face),"Cannot open small bitmap fixture");
    require(!FT_IS_SCALABLE(face)&&face->num_fixed_sizes&&FT_Get_Char_Index(face,65)&&FT_Get_Char_Index(face,233),"Small fixture must otherwise qualify as Unicode bitmap text");
    addBitmapFont(face,smallFont,{});
    require(bitmapFonts.size()==before,"Sub-12px bitmap font must not enter the picker");
    require(!QFontDatabase::families().contains("DomeSmallTest"),"Rejected small font must not be registered for fallback");
    FT_Done_Face(face);FT_Done_FreeType(library);FcPatternDestroy(match);FcPatternDestroy(query);
    QJsonObject appearance{{"font",first["id"]},{"font_size",16},{"opacity",248}};
    auto font=interfaceFont(appearance);auto raw=QRawFont::fromFont(font);
    require(font.pixelSize()==16&&!QFontDatabase::isSmoothlyScalable(font.family(),font.styleName()),"Selection must use a native bitmap strike");
    require(raw.familyName()==first["family"].toString(),"Selected bitmap font must not be substituted");
    for(auto cp:{0x00e9u,0x0159u,0x00dfu,0x03b1u,0x0416u,0x306bu,0x65e5u,0xac00u,0x0e01u,0x0627u,0xfe8eu})require(raw.supportsCharacter(cp),"Unicode glyph missing");
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
