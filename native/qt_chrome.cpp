// SPDX-License-Identifier: MIT
// Qt is a paint/style adapter. Window management and input remain Rust-owned.
#include <QtWidgets/QApplication>
#include <QtWidgets/QStyleFactory>
#include <QtWidgets/QStyleOptionTitleBar>
#include <QtWidgets/QStyleOptionMenuItem>
#include <QtGui/QPainter>
#include <QtGui/QIcon>
#include <QtWidgets/QStyleOption>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonObject>
#include <QtCore/QJsonArray>
#include <QtCore/QDateTime>
#include <cstring>
static QApplication *app() {
    static int argc=3;static char name[]="dome-qt",platform[]="-platform",offscreen[]="offscreen";
    static char *argv[]={name,platform,offscreen,nullptr};
    static QApplication *application=[](){auto *a=new QApplication(argc,argv);a->setStyle(QStyleFactory::create("Fusion"));a->setFont(QFont("Sans Serif",10));return a;}();return application;
}
static QStyleOptionTitleBar title(int width,bool active,const QString &text) {
    QStyleOptionTitleBar s;s.rect=QRect(0,0,width,30);s.text=text;s.titleBarFlags=Qt::Window|Qt::WindowTitleHint|Qt::WindowSystemMenuHint|Qt::WindowMinimizeButtonHint|Qt::WindowMaximizeButtonHint|Qt::WindowCloseButtonHint;
    s.state=QStyle::State_Enabled|(active?QStyle::State_Active:QStyle::State_None);s.titleBarState=active?Qt::WindowActive:Qt::WindowNoState;s.palette=app()->palette();
    s.subControls=QStyle::SC_TitleBarLabel|QStyle::SC_TitleBarMinButton|QStyle::SC_TitleBarMaxButton|QStyle::SC_TitleBarCloseButton;return s;
}
extern "C" int dome_qt_hit(int width,int height,int x,int y) {
    app();if(x<4||x>=width-4||y<4||y>=height-4)return 5;if(y<30){auto s=title(width,true,{});auto h=app()->style()->hitTestComplexControl(QStyle::CC_TitleBar,&s,QPoint(x,y));if(h==QStyle::SC_TitleBarCloseButton)return 1;if(h==QStyle::SC_TitleBarMinButton)return 2;if(h==QStyle::SC_TitleBarMaxButton)return 3;return 4;}
    if(x<4||x>=width-4||y>=height-4)return 5;return 0;
}
extern "C" int dome_qt_paint(const char *json,unsigned char *pixels,int length) {
    app();auto o=QJsonDocument::fromJson(json).object();int w=o["width"].toInt(),h=o["height"].toInt();if(w<1||h<1||qint64(w)*h*4!=length)return 0;
    QImage image(pixels,w,h,QImage::Format_ARGB32_Premultiplied);image.fill(Qt::transparent);QPainter p(&image);p.setRenderHint(QPainter::Antialiasing);p.setFont(app()->font());
    if(o["kind"].toString()=="frame") {
        p.fillRect(image.rect(),QColor(92,98,104));p.setCompositionMode(QPainter::CompositionMode_Clear);p.fillRect(QRect(4,30,w-8,h-34),Qt::transparent);p.setCompositionMode(QPainter::CompositionMode_SourceOver);
        auto s=title(w,o["active"].toBool(),o["title"].toString());app()->style()->drawComplexControl(QStyle::CC_TitleBar,&s,&p);return 1;
    }
    // Classic global menu hierarchy: system, bold application, commands, right extras.
    // Both bars and their menus share one palette, including opacity and selection.
    auto appearance=o["appearance"].toObject();bool light=appearance["light"].toBool(false);
    QColor background=light?QColor(229,233,239):QColor(31,37,45);
    QColor foreground=light?QColor(30,37,46):QColor(231,235,241);
    QColor muted=light?QColor(104,116,132):QColor(146,154,165);
    QColor selection=light?QColor(199,213,232):QColor(63,77,96);
    QColor accent(appearance["accent"].toString("#6babff"));
    QColor barBackground=background;barBackground.setAlpha(appearance["opacity"].toInt(248));
    QPalette menuPalette=app()->palette();
    for(auto group:{QPalette::Active,QPalette::Inactive,QPalette::Disabled}){
        auto text=group==QPalette::Disabled?muted:foreground;
        for(auto role:{QPalette::Window,QPalette::Button,QPalette::Base})menuPalette.setColor(group,role,background);
        for(auto role:{QPalette::WindowText,QPalette::ButtonText,QPalette::Text})menuPalette.setColor(group,role,text);
        menuPalette.setColor(group,QPalette::Highlight,selection);menuPalette.setColor(group,QPalette::HighlightedText,foreground);
        menuPalette.setColor(group,QPalette::Mid,muted);
    }
    p.fillRect(0,0,w,36,barBackground);p.setPen(muted);p.drawLine(0,35,w,35);
    QStringList labels={QString::fromUtf8("◉"),o["app"].toString(),"File","Edit","View","Window","Help"};int xs[]={0,46,230,280,332,390,470};int widths[]={46,184,50,52,58,80,58};int menu=o["menu"].toInt(-1);
    for(int i=0;i<labels.size();++i){QRect rect(xs[i],0,widths[i],35);QFont font=app()->font();font.setBold(i==1);p.setFont(font);if(i==menu)p.fillRect(rect,selection);p.setPen(foreground);p.drawText(rect,Qt::AlignCenter,p.fontMetrics().elidedText(labels[i],Qt::ElideRight,widths[i]-14));}
    p.setFont(app()->font());p.setPen(foreground);p.drawText(QRect(w-168,0,152,35),Qt::AlignRight|Qt::AlignVCenter,QDateTime::currentDateTime().toString("ddd HH:mm"));
    auto tray=o["tray"].toObject();auto extras=tray["items"].toArray();int tx=w-176-extras.size()*28;
    for(int index=0;index<extras.size();++index){auto item=extras[index].toObject();QImage status(22,22,QImage::Format_ARGB32_Premultiplied);status.fill(Qt::transparent);QPainter iconPainter(&status);auto icon=QIcon::fromTheme(item["icon"].toString());if(!icon.isNull()){icon.paint(&iconPainter,QRect(0,0,22,22));}else{auto raw=QByteArray::fromBase64(item["pixmap"].toString().toLatin1());int iw=item["width"].toInt(),ih=item["height"].toInt();if(iw>0&&ih>0&&raw.size()==qint64(iw)*ih*4){QByteArray rgba=raw;for(int k=0;k<raw.size();k+=4){rgba[k]=raw[k+1];rgba[k+1]=raw[k+2];rgba[k+2]=raw[k+3];rgba[k+3]=raw[k];}QImage bitmap(reinterpret_cast<const unsigned char*>(rgba.constData()),iw,ih,QImage::Format_RGBA8888);iconPainter.drawImage(QRect(0,0,22,22),bitmap);}}
        iconPainter.setCompositionMode(QPainter::CompositionMode_SourceIn);iconPainter.fillRect(status.rect(),foreground);iconPainter.end();int x=tx+index*28;if(tray["menu"].toInt(-1)==index){p.fillRect(QRect(x,0,28,35),selection);}p.drawImage(x+3,7,status);if(item["attention"].toBool()){p.setPen(accent);p.drawLine(x+10,32,x+18,32);}}
    p.fillRect(0,h-60,w,60,barBackground);p.setPen(foreground);p.drawText(QRect(12,h-60,86,60),Qt::AlignVCenter,"⌕  Launch");p.drawText(QRect(w-150,h-60,140,60),Qt::AlignVCenter,"▦  Desktops");
    int i=0;for(auto v:o["tasks"].toArray()){auto task=v.toObject();int x=110+i++*130;if(x>w-280)break;
        if(task["active"].toBool())p.fillRect(QRect(x-5,h-56,126,52),selection);
        QIcon icon=QIcon::fromTheme(task["icon"].toString());if(!icon.isNull())icon.paint(&p,QRect(x+4,h-46,28,28));
        p.setPen(task["minimized"].toBool()?muted:foreground);p.drawText(QRect(x+37,h-51,82,36),Qt::AlignVCenter,p.fontMetrics().elidedText(task["name"].toString(),Qt::ElideRight,80));
        int count=task["windows"].toArray().size();if(count>0){p.setPen(Qt::NoPen);p.setBrush(task["active"].toBool()?accent:QColor(155,169,188));p.drawRoundedRect(QRect(x+24,h-7,76,3),1,1);}if(count>1){p.setPen(foreground);p.drawText(QRect(x+96,h-18,22,14),Qt::AlignRight,QString::number(count));}
    }
    if(o["launcher"].toBool()){p.fillRect(QRect(20,56,600,400),QColor(26,35,45));p.setPen(QColor(232,238,245));p.drawText(36,88,"Applications  ·  "+o["query"].toString());i=0;for(auto value:o["apps"].toArray()){auto a=value.toObject();int y=106+i++*36;QIcon icon=QIcon::fromTheme(a["icon"].toString());icon.paint(&p,QRect(36,y,24,24));p.drawText(72,y+19,p.fontMetrics().elidedText(a["name"].toString(),Qt::ElideRight,280));auto categories=a["categories"].toArray();if(!categories.isEmpty()){p.setPen(QColor(155,172,187));p.drawText(QRect(370,y,230,26),Qt::AlignRight|Qt::AlignVCenter,categories[0].toString());p.setPen(QColor(232,238,245));}}}
    if(o["manager"].toBool()){p.fillRect(QRect(w-330,h-390,320,320),QColor(26,35,45));p.setPen(QColor(232,238,245));i=0;for(auto a:o["desktops"].toArray()){p.drawText(w-310,h-362+i*32,QString::number(i+1)+"  "+a.toString());++i;}p.drawText(w-310,h-362+i*32,"+ New desktop");}
    auto popup=o["task_popup"].toObject();if(!popup.isEmpty()){int x=popup["x"].toInt(),y=popup["y"].toInt(),pw=popup["w"].toInt(),ph=popup["h"].toInt();p.fillRect(QRect(x,y,pw,ph),QColor(38,45,55,254));auto rows=popup["rows"].toArray();i=0;for(auto value:rows){auto a=value.toObject();bool context=popup["context"].toBool();int ry=y+6+i++*(context?32:88);if(context){p.setPen(a["enabled"].toBool()?QColor(238,242,247):QColor(129,137,149));p.drawText(QRect(x+14,ry,pw-28,32),Qt::AlignVCenter,a["text"].toString());}else{
        if(a["active"].toBool())p.fillRect(QRect(x+4,ry,pw-8,84),QColor(64,80,102));int iw=a["width"].toInt(),ih=a["height"].toInt();auto bytes=QByteArray::fromBase64(a["rgba"].toString().toLatin1());if(iw>0&&ih>0&&bytes.size()==qint64(iw)*ih*4){QImage thumb(reinterpret_cast<const unsigned char*>(bytes.constData()),iw,ih,QImage::Format_RGBA8888);QSize size=thumb.size().scaled(126,76,Qt::KeepAspectRatio);p.drawImage(QRect(x+10,ry+4,size.width(),size.height()),thumb);}else{p.fillRect(QRect(x+10,ry+4,126,76),QColor(66,75,88));}
        p.setPen(QColor(238,242,247));p.drawText(QRect(x+146,ry+5,162,70),Qt::AlignVCenter|Qt::TextWordWrap,p.fontMetrics().elidedText(a["text"].toString(),Qt::ElideRight,155));
    }}}
    if(menu>=0){auto items=o["items"].toArray();int x=xs[menu],height=items.size()*28+8;p.fillRect(QRect(x,36,380,height),background);for(i=0;i<items.size();++i){auto a=items[i].toObject();QStyleOptionMenuItem item;item.rect=QRect(x+3,40+i*28,374,28);item.text=a["text"].toString();item.state=a["enabled"].toBool(true)?QStyle::State_Enabled:QStyle::State_None;item.menuItemType=a["separator"].toBool()?QStyleOptionMenuItem::Separator:QStyleOptionMenuItem::Normal;if(i==o["hover"].toInt(-1))item.state|=QStyle::State_Selected;item.palette=menuPalette;app()->style()->drawControl(QStyle::CE_MenuItem,&item,&p);p.setPen(a["enabled"].toBool()?foreground:muted);if(i==o["hover"].toInt(-1)&&a["enabled"].toBool())p.setPen(foreground);p.drawText(QRect(x+240,40+i*28,128,28),Qt::AlignRight|Qt::AlignVCenter,a["shortcut"].toString());}}
    if(!tray["menu"].isNull()&&!tray["menu"].isUndefined()){auto rows=tray["rows"].toArray();int index=tray["menu"].toInt();int x=qBound(4,w-176-(extras.size()-index)*28,w-284);p.fillRect(QRect(x,36,280,rows.size()*28+8),background);for(int n=0;n<rows.size();++n){auto row=rows[n].toObject();QStyleOptionMenuItem item;item.rect=QRect(x+3,40+n*28,274,28);item.text=row["text"].toString();item.state=row["enabled"].toBool()?QStyle::State_Enabled:QStyle::State_None;item.menuItemType=row["separator"].toBool()?QStyleOptionMenuItem::Separator:(row["submenu"].toBool()?QStyleOptionMenuItem::SubMenu:QStyleOptionMenuItem::Normal);item.checked=row["checked"].toBool();item.checkType=row["checkable"].toBool()?QStyleOptionMenuItem::NonExclusive:QStyleOptionMenuItem::NotCheckable;item.palette=menuPalette;if(!row["enabled"].toBool())item.palette.setCurrentColorGroup(QPalette::Disabled);app()->style()->drawControl(QStyle::CE_MenuItem,&item,&p);}}
    if(o["about"].toBool()){QRect box((w-440)/2,(h-200)/2,440,200);p.fillRect(box,background);p.setPen(foreground);auto info=o["app_info"].toObject();QString text=o["about_is_app"].toBool()?info["name"].toString()+"\n"+info["comment"].toString()+"\n"+info["desktop_id"].toString():"Dome\nExperimental Rust / Smithay compositor\nQt6 desktop chrome · Native OpenXR";p.drawText(box.adjusted(24,24,-24,-24),Qt::AlignCenter|Qt::TextWordWrap,text+"\n\nClick or press a key to close");}
    if(o["appearance_open"].toBool()){
        QRect box((w-600)/2,(h-400)/2,600,400);p.fillRect(box,background);p.setPen(foreground);
        p.drawText(box.adjusted(24,18,-24,-340),Qt::AlignLeft|Qt::AlignVCenter,"Appearance & Input");
        QStringList names={"Colour scheme","Accent colour","Desktop background","Bar opacity","Resume hand cursor after","Reset settings"};
        QStringList values={light?"Light":"Dark",accent.name(),"Change",QString::number(barBackground.alpha()*100/255)+"%",QString::number(appearance["hand_resume_ms"].toInt(800))+" ms","Reset"};
        for(int n=0;n<names.size();++n){int y=box.y()+60+n*48;p.setPen(foreground);p.drawText(QRect(box.x()+24,y,270,40),Qt::AlignVCenter,names[n]);p.fillRect(QRect(box.x()+300,y,276,40),selection);p.drawText(QRect(box.x()+300,y,276,40),Qt::AlignCenter,values[n]);}
        p.setPen(muted);p.drawText(QRect(box.x()+24,box.y()+350,552,36),Qt::AlignCenter,"Changes save automatically · Click here or press Esc to close");
    }
    if(!o["session_confirm"].isNull()&&!o["session_confirm"].isUndefined()){
        QRect box((w-440)/2,(h-200)/2,440,200);p.fillRect(box,background);p.setPen(foreground);
        auto label=o["session_confirm"].toString();p.drawText(box.adjusted(24,20,-24,-110),Qt::AlignCenter,label+"?");
        p.setPen(muted);p.drawText(box.adjusted(24,70,-24,-60),Qt::AlignCenter|Qt::TextWordWrap,label=="Log Out of Dome"?"Close this Dome compositor session and its application connections.":"This action applies to the computer session.");
        p.fillRect(QRect(box.x()+24,box.y()+140,180,40),selection);p.fillRect(QRect(box.x()+230,box.y()+140,186,40),selection);p.setPen(foreground);p.drawText(QRect(box.x()+24,box.y()+140,180,40),Qt::AlignCenter,"Cancel");p.drawText(QRect(box.x()+230,box.y()+140,186,40),Qt::AlignCenter,label);
    }
    auto error=o["session_error"].toString();if(!error.isEmpty()){p.fillRect(QRect(w/2-300,h-106,600,40),background);p.setPen(foreground);p.drawText(QRect(w/2-290,h-106,580,40),Qt::AlignVCenter,p.fontMetrics().elidedText(error,Qt::ElideRight,580));}
    return 1;
}
