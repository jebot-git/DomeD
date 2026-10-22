// SPDX-License-Identifier: MIT
// Qt is a paint/style adapter. Window management and input remain Rust-owned.
#include <QtWidgets/QApplication>
#include <QtWidgets/QStyleFactory>
#include <QtWidgets/QStyleOptionMenuItem>
#include <QtGui/QPainter>
#include <QtGui/QIcon>
#include <QtWidgets/QStyleOption>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonObject>
#include <QtCore/QJsonArray>
#include <QtCore/QDateTime>
#include <QtCore/QDir>
#include <QtCore/QFileInfo>
#include <cstring>
#include "keyboard-flags.h"
static QApplication *app() {
    static int argc=5;static char name[]="dome-qt",platform[]="-platform",offscreen[]="offscreen",theme[]="-platformtheme",generic[]="generic";
    static char *argv[]={name,platform,offscreen,theme,generic,nullptr};
    static QApplication *application=[](){auto *a=new QApplication(argc,argv);a->setStyle(QStyleFactory::create("Fusion"));a->setFont(QFont("Sans Serif",10));if(QIcon::themeName().isEmpty())QIcon::setThemeName("breeze");QIcon::setFallbackThemeName("hicolor");return a;}();return application;
}
#include "bitmap_fonts.hpp"
// Shared by painting and hit testing: close left, zoom then depth at right.
static QRect workbenchGadget(int width,int hit,int caption=30) {
    if(hit==1)return QRect(0,0,30,caption);
    if(hit==3)return QRect(width-60,0,30,caption);
    if(hit==6)return QRect(width-30,0,30,caption);
    return {};
}
extern "C" int dome_qt_hit(int width,int height,int x,int y,int caption=30) {
    if(x<0||y<0||x>=width||y>=height)return 0;
    if(y<caption){for(int hit:{1,3,6})if(workbenchGadget(width,hit,caption).contains(x,y))return hit;return 4;}
    if(x<4||x>=width-4||y>=height-4)return 5;
    return 0;
}
// SNI pixmaps are network-order ARGB, not premultiplied RGBA. Preserve their colours.
static QImage trayImage(const QJsonObject &item,const QColor &foreground) {
    QImage result(22,22,QImage::Format_ARGB32_Premultiplied);result.fill(Qt::transparent);
    QPainter p(&result);p.setRenderHint(QPainter::Antialiasing);p.setRenderHint(QPainter::SmoothPixmapTransform);
    const auto key=item["key"].toString();
    if(key=="dome.audio") {
        QColor colour=foreground;if(item["icon"].toString()=="unavailable")colour.setAlpha(130);
        p.setPen(QPen(colour,1.6));p.setBrush(Qt::NoBrush);p.drawPolygon(QPolygonF{QPointF(3,9),QPointF(7,9),QPointF(12,5),QPointF(12,17),QPointF(7,13),QPointF(3,13)});
        if(item["icon"].toString()=="muted"||item["icon"].toString()=="unavailable"){p.drawLine(16,8,21,14);p.drawLine(16,14,21,8);}
        else {p.drawArc(QRectF(10,7,8,8),-60*16,120*16);p.drawArc(QRectF(9,4,13,14),-60*16,120*16);}
        return result;
    }
    if(key=="dome.clipboard"||key=="dome.notifications") {
        p.setPen(QPen(foreground,1.6));p.setBrush(Qt::NoBrush);
        if(key=="dome.clipboard") {p.drawRoundedRect(QRectF(5,4,12,15),1.5,1.5);p.drawRoundedRect(QRectF(8,2,6,4),1,1);p.drawLine(8,10,14,10);p.drawLine(8,14,14,14);}
        else {p.drawArc(QRectF(6,4,10,12),0,180*16);p.drawLine(6,10,5,16);p.drawLine(16,10,17,16);p.drawLine(5,16,17,16);p.drawArc(QRectF(9,16,4,4),180*16,180*16);p.drawLine(11,2,11,4);}
        return result;
    }
    auto raw=QByteArray::fromBase64(item["pixmap"].toString().toLatin1());int w=item["width"].toInt(),h=item["height"].toInt();
    if(w>0&&h>0&&w<=512&&h<=512&&raw.size()==qint64(w)*h*4) {
        QByteArray rgba=raw;for(int k=0;k<raw.size();k+=4){rgba[k]=raw[k+1];rgba[k+1]=raw[k+2];rgba[k+2]=raw[k+3];rgba[k+3]=raw[k];}
        QImage bitmap(reinterpret_cast<const unsigned char*>(rgba.constData()),w,h,QImage::Format_RGBA8888);
        QSize size=bitmap.size().scaled(22,22,Qt::KeepAspectRatio);p.drawImage(QRect(QPoint((22-size.width())/2,(22-size.height())/2),size),bitmap);
        return result;
    }
    const auto name=item["icon"].toString(),path=item["icon_theme_path"].toString();auto paths=QIcon::themeSearchPaths();
    if(!path.isEmpty())QIcon::setThemeSearchPaths(QStringList{path}+paths);
    QIcon icon=QFileInfo(name).isAbsolute()?QIcon(name):QIcon::fromTheme(name);
    if(icon.isNull()&&!path.isEmpty())for(const auto &suffix:QStringList{"",".svg",".png",".xpm"}) {auto file=QDir(path).filePath(name+suffix);if(QFileInfo::exists(file)){icon=QIcon(file);break;}}
    if(!icon.isNull())icon.paint(&p,QRect(0,0,22,22));
    else {p.setPen(foreground);p.drawRoundedRect(QRect(3,3,16,16),3,3);p.drawText(QRect(0,0,22,22),Qt::AlignCenter,item["title"].toString().left(1).toUpper());}
    if(!path.isEmpty())QIcon::setThemeSearchPaths(paths);
    return result;
}
extern "C" int dome_qt_paint(const char *json,unsigned char *pixels,int length) {
    app();auto o=QJsonDocument::fromJson(json).object();int w=o["width"].toInt(),h=o["height"].toInt();if(w<1||h<1||qint64(w)*h*4!=length)return 0;
    QFont font=interfaceFont(o["appearance"].toObject());if(app()->font()!=font)app()->setFont(font);
    QImage image(pixels,w,h,QImage::Format_ARGB32_Premultiplied);image.fill(Qt::transparent);QPainter p(&image);p.setRenderHint(QPainter::Antialiasing);p.setFont(app()->font());
    // Classic global menu hierarchy: system, bold application, commands, right extras.
    // Both bars and their menus share one palette, including opacity and selection.
    auto appearance=o["appearance"].toObject();bool light=appearance["light"].toBool(false);
    QColor background=light?QColor(229,233,239):QColor(31,37,45);
    QColor foreground=light?QColor(30,37,46):QColor(231,235,241);
    QColor muted=light?QColor(104,116,132):QColor(146,154,165);
    QColor selection=light?QColor(199,213,232):QColor(63,77,96);
    QColor accent(appearance["accent"].toString("#6babff"));
    QColor barBackground=background;barBackground.setAlpha(appearance["opacity"].toInt(255));
    QPalette menuPalette=app()->palette();
    for(auto group:{QPalette::Active,QPalette::Inactive,QPalette::Disabled}){
        auto text=group==QPalette::Disabled?muted:foreground;
        for(auto role:{QPalette::Window,QPalette::Button,QPalette::Base})menuPalette.setColor(group,role,background);
        for(auto role:{QPalette::WindowText,QPalette::ButtonText,QPalette::Text})menuPalette.setColor(group,role,text);
        menuPalette.setColor(group,QPalette::Highlight,selection);menuPalette.setColor(group,QPalette::HighlightedText,foreground);
        menuPalette.setColor(group,QPalette::Mid,muted);
    }
    if(o["kind"].toString()=="frame") {
        int captionHeight=qMax(30,appearance["font_size"].toInt(16)*appearance["font_scale"].toInt(1)+8);
        bool active=o["active"].toBool();
        QColor border=active?accent:muted;border.setAlpha(active?96:64);
        p.fillRect(QRect(0,captionHeight,4,h-captionHeight),border);p.fillRect(QRect(w-4,captionHeight,4,h-captionHeight),border);p.fillRect(QRect(0,h-4,w,4),border);
        QColor caption=active?(light?accent.lighter(145):accent.darker(190)):background;
        p.setOpacity(appearance["opacity"].toInt(255)/255.0);
        p.fillRect(QRect(0,0,w,captionHeight),caption);
        QRect label(38,0,w-106,captionHeight);
        p.setPen(foreground);p.drawText(label,Qt::AlignLeft|Qt::AlignVCenter,p.fontMetrics().elidedText(o["title"].toString(),Qt::ElideRight,label.width()));
        // Workbench relief, square close gadget, size toggle and overlapping depth planes.
        p.setRenderHint(QPainter::Antialiasing,false);
        for(int hit:{1,3,6}){
            QRect button=workbenchGadget(w,hit,captionHeight).adjusted(1,1,-1,-1);
            bool down=o["pressed"].toInt()==hit;
            p.fillRect(button,down?selection:caption);
            p.setPen(down?caption.darker(180):caption.lighter(160));
            p.drawLine(button.topLeft(),button.topRight());p.drawLine(button.topLeft(),button.bottomLeft());
            p.setPen(down?caption.lighter(160):caption.darker(180));
            p.drawLine(button.bottomLeft(),button.bottomRight());p.drawLine(button.topRight(),button.bottomRight());
            QPoint center=button.center()+QPoint(down?1:0,down?1:0);
            p.setPen(QPen(active?foreground:muted,1));p.setBrush(Qt::NoBrush);
            if(hit==1){p.fillRect(QRect(center.x()-4,center.y()-4,9,9),active?foreground:muted);}
            else if(hit==3){
                p.drawRect(QRect(center.x()-7,center.y()-5,14,10));
                if(o["zoomed"].toBool())p.fillRect(QRect(center.x()-5,center.y()-3,5,4),active?foreground:muted);
                else p.drawRect(QRect(center.x()-5,center.y()-3,5,4));
            } else {
                p.drawRect(QRect(center.x()-7,center.y()-5,10,8));
                p.fillRect(QRect(center.x()-2,center.y()-1,10,8),down?selection:caption);
                p.drawRect(QRect(center.x()-2,center.y()-1,10,8));
            }
        }
        return 1;
    }
    if(o["kind"].toString()!="notifications") {
    p.fillRect(0,0,w,36,barBackground);p.setPen(muted);p.drawLine(0,35,w,35);
    QStringList labels={QString::fromUtf8("◉"),o["app"].toString(),"File","Edit","View","Window","Help"};int xs[]={0,46,230,280,332,390,470};int widths[]={46,184,50,52,58,80,58};int menu=o["menu"].toInt(-1);
    auto headers=o["menu_headers"].toArray();
    if(headers.isEmpty())for(int i=0;i<labels.size();++i)headers.append(QJsonObject{{"text",labels[i]},{"x",xs[i]},{"width",widths[i]},{"enabled",true}});
    auto clock=o["clock"].toObject();int clockWidth=clock["width"].toInt(176),reserve=clockWidth+44;
    int menuRight=w-reserve-o["tray"].toObject()["items"].toArray().size()*28;
    p.save();p.setClipRect(QRect(0,0,qMax(0,menuRight),36));
    for(int i=0;i<headers.size();++i){auto header=headers[i].toObject();int width=header["width"].toInt();QRect rect(header["x"].toInt(),0,width,35);QFont font=app()->font();font.setBold(i==1);p.setFont(font);if(i==menu)p.fillRect(rect,selection);p.setPen(header["enabled"].toBool()?foreground:muted);auto iconName=header["icon"].toString();if(!iconName.isEmpty()){QIcon icon=QFileInfo(iconName).isAbsolute()?QIcon(iconName):QIcon::fromTheme(iconName);icon.paint(&p,QRect(rect.x()+5,6,24,24));rect.adjust(36,0,-7,0);}p.drawText(rect,Qt::AlignCenter,p.fontMetrics().elidedText(header["text"].toString(),Qt::ElideRight,rect.width()-14));}
    p.restore();
    p.setFont(app()->font());p.setPen(foreground);
    auto secondary=clock["secondary"].toString();
    if(clock["popup"].isObject())p.fillRect(QRect(w-clockWidth,0,clockWidth,35),selection);
    p.drawText(QRect(w-clockWidth+8,0,clockWidth-24,secondary.isEmpty()?35:17),Qt::AlignRight|Qt::AlignVCenter,clock.isEmpty()?QDateTime::currentDateTime().toString("ddd HH:mm"):clock["text"].toString());
    if(!secondary.isEmpty())p.drawText(QRect(w-clockWidth+8,17,clockWidth-24,18),Qt::AlignRight|Qt::AlignVCenter,secondary);
    QPixmap keyboardFlag;
    keyboardFlag.loadFromData(QByteArray::fromStdString(dx_flags::svg(o["keyboard_flag"].toString("generic").toStdString(),foreground.name().toStdString())),"SVG");
    if(o["tray"].toObject()["keyboard_menu"].toBool()&&!o["tray"].toObject()["menu"].isNull())p.fillRect(QRect(w-reserve,0,44,35),selection);
    if(!keyboardFlag.isNull())p.drawPixmap(QRect(w-reserve,6,24,24),keyboardFlag);
    p.drawText(QRect(w-reserve+25,0,19,35),Qt::AlignVCenter,o["keyboard"].toString());
    auto tray=o["tray"].toObject();auto extras=tray["items"].toArray();int tx=w-reserve-extras.size()*28;
    for(int index=0;index<extras.size();++index){auto item=extras[index].toObject();int x=tx+index*28;
        if((!tray["keyboard_menu"].toBool()&&!tray["menu"].isNull()&&tray["menu"].toInt(-1)==index)||(item["key"].toString()=="dome.audio"&&o["audio"].isObject())||(item["key"].toString()=="dome.clipboard"&&o["history"].toObject()["kind"].toString()=="clipboard")||(item["key"].toString()=="dome.notifications"&&o["history"].toObject()["kind"].toString()=="notifications"))p.fillRect(QRect(x,0,28,35),selection);
        p.drawImage(x+3,7,trayImage(item,foreground));if(item["attention"].toBool()){p.setPen(accent);p.drawLine(x+10,32,x+18,32);}
    }
    p.fillRect(0,h-60,w,60,barBackground);p.setPen(foreground);p.drawText(QRect(12,h-60,86,60),Qt::AlignVCenter,"⌕  Launch");p.drawText(QRect(w-240,h-60,140,60),Qt::AlignVCenter,"▦  Desktops");p.drawText(QRect(w-90,h-60,86,60),Qt::AlignCenter,o["xr_active"].toBool()?"Desktop":"XR");
    int i=0;for(auto v:o["tasks"].toArray()){auto task=v.toObject();int x=110+i++*130;if(x>w-380)break;
        if(task["active"].toBool())p.fillRect(QRect(x-5,h-56,126,52),selection);
        QIcon icon=QIcon::fromTheme(task["icon"].toString());if(!icon.isNull())icon.paint(&p,QRect(x+4,h-46,28,28));
        p.setPen(task["minimized"].toBool()?muted:foreground);p.drawText(QRect(x+37,h-51,82,36),Qt::AlignVCenter,p.fontMetrics().elidedText(task["name"].toString(),Qt::ElideRight,80));
        int count=task["windows"].toArray().size();if(count>0){p.setPen(Qt::NoPen);p.setBrush(task["active"].toBool()?accent:QColor(155,169,188));p.drawRoundedRect(QRect(x+24,h-7,76,3),1,1);}if(count>1){p.setPen(foreground);p.drawText(QRect(x+96,h-18,22,14),Qt::AlignRight,QString::number(count));}
    }
    if(o["launcher"].toBool()) {
        p.fillRect(QRect(20,56,600,400),QColor(26,35,45));
        p.setPen(QColor(232,238,245));
        p.drawText(36,88,p.fontMetrics().elidedText("Applications  ·  "+o["query"].toString(),Qt::ElideLeft,560));
        i=0;
        for(auto value:o["apps"].toArray()) {
            auto a=value.toObject();int y=106+i*36;
            if(i++==o["launch_index"].toInt())p.fillRect(QRect(28,y-2,584,34),selection);
            QIcon icon=QIcon::fromTheme(a["icon"].toString());icon.paint(&p,QRect(36,y,24,24));
            p.drawText(72,y+19,p.fontMetrics().elidedText(a["name"].toString(),Qt::ElideRight,280));
            auto categories=a["categories"].toArray();
            if(!categories.isEmpty()) {
                p.setPen(QColor(155,172,187));
                p.drawText(QRect(370,y,230,26),Qt::AlignRight|Qt::AlignVCenter,categories[0].toString());
                p.setPen(QColor(232,238,245));
            }
        }
        p.setPen(QColor(155,172,187));
        const int total=o["launch_total"].toInt(),offset=o["launch_offset"].toInt();
        const QString status=total ? QString("%1–%2 of %3  ·  ↑/↓ browse  ·  Type to filter").arg(offset+1).arg(offset+i).arg(total)
                                   : QString("No menu matches  ·  Enter an executable name to run it");
        p.drawText(36,438,p.fontMetrics().elidedText(status,Qt::ElideRight,560));
        p.setPen(QColor(255,150,150));
        p.drawText(36,412,p.fontMetrics().elidedText(o["launch_error"].toString(),Qt::ElideRight,560));
    }
    if(o["manager"].toBool()){p.fillRect(QRect(w-330,h-390,320,320),QColor(26,35,45));p.setPen(QColor(232,238,245));i=0;for(auto a:o["desktops"].toArray()){if(i==o["shell_selection"].toInt())p.fillRect(QRect(w-320,h-384+i*32,300,32),selection);p.drawText(w-310,h-362+i*32,QString::number(i+1)+"  "+a.toString());++i;}if(i==o["shell_selection"].toInt())p.fillRect(QRect(w-320,h-384+i*32,300,32),selection);p.drawText(w-310,h-362+i*32,"+ New desktop");}
    auto popup=o["task_popup"].toObject();if(!popup.isEmpty()){int x=popup["x"].toInt(),y=popup["y"].toInt(),pw=popup["w"].toInt(),ph=popup["h"].toInt();p.fillRect(QRect(x,y,pw,ph),QColor(38,45,55,254));int band=popup["scroll_band"].toInt(),offset=popup["offset"].toInt();p.save();p.setClipRect(QRect(x,y+band,pw,ph-2*band));auto rows=popup["rows"].toArray();i=0;for(auto value:rows){auto a=value.toObject();bool context=popup["context"].toBool();int index=i;int ry=y+band+6+i++*(context?32:88)-offset;if(context){if(index==popup["selected"].toInt(-1))p.fillRect(QRect(x+4,ry,pw-8,32),selection);p.setPen(a["enabled"].toBool()?QColor(238,242,247):QColor(129,137,149));p.drawText(QRect(x+14,ry,pw-28,32),Qt::AlignVCenter,a["text"].toString());}else{
        if(index==popup["selected"].toInt(-1)||a["active"].toBool()){p.fillRect(QRect(x+4,ry,pw-8,84),QColor(64,80,102));}int iw=a["width"].toInt(),ih=a["height"].toInt();auto bytes=QByteArray::fromBase64(a["rgba"].toString().toLatin1());if(iw>0&&ih>0&&bytes.size()==qint64(iw)*ih*4){QImage thumb(reinterpret_cast<const unsigned char*>(bytes.constData()),iw,ih,QImage::Format_RGBA8888);QSize size=thumb.size().scaled(126,76,Qt::KeepAspectRatio);p.drawImage(QRect(x+10,ry+4,size.width(),size.height()),thumb);}else{p.fillRect(QRect(x+10,ry+4,126,76),QColor(66,75,88));}
        p.setPen(QColor(238,242,247));p.drawText(QRect(x+146,ry+5,162,70),Qt::AlignVCenter|Qt::TextWordWrap,p.fontMetrics().elidedText(a["text"].toString(),Qt::ElideRight,155));
    }}p.restore();if(band>0){for(bool up:{true,false}){QRect control(x,up?y:y+ph-band,pw,band);p.fillRect(control,background);p.setPen(popup[up?"can_up":"can_down"].toBool()?foreground:muted);p.drawText(control,Qt::AlignCenter,up?QString::fromUtf8("▲"):QString::fromUtf8("▼"));}}}
    if(menu>=0&&!o["items"].toArray().isEmpty()){auto items=o["items"].toArray();int x=headers[menu].toObject()["x"].toInt(),height=items.size()*28+8;p.fillRect(QRect(x,36,380,height),background);for(i=0;i<items.size();++i){auto a=items[i].toObject();QStyleOptionMenuItem item;item.rect=QRect(x+3,40+i*28,374,28);item.text=a["text"].toString();item.state=a["enabled"].toBool(true)?QStyle::State_Enabled:QStyle::State_None;item.menuItemType=a["separator"].toBool()?QStyleOptionMenuItem::Separator:QStyleOptionMenuItem::Normal;if(i==o["hover"].toInt(-1))item.state|=QStyle::State_Selected;item.palette=menuPalette;app()->style()->drawControl(QStyle::CE_MenuItem,&item,&p);p.setPen(a["enabled"].toBool()?foreground:muted);if(i==o["hover"].toInt(-1)&&a["enabled"].toBool())p.setPen(foreground);p.drawText(QRect(x+240,40+i*28,128,28),Qt::AlignRight|Qt::AlignVCenter,a["shortcut"].toString());}}
    for(auto value:o["appmenu_panels"].toArray()){
        auto panel=value.toObject();int x=panel["x"].toInt(),y=panel["y"].toInt(),pw=panel["width"].toInt();auto rows=panel["rows"].toArray();
        p.save();p.setClipRect(QRect(0,36,w,h-36));p.fillRect(QRect(x,y,pw,rows.size()*28+8),background);
        p.setBrush(Qt::NoBrush);p.setPen(muted);p.drawRect(QRect(x,y,pw-1,rows.size()*28+7));
        for(int n=0;n<rows.size();++n){
            auto row=rows[n].toObject();int ry=y+4+n*28;
            if(row["separator"].toBool()){p.setPen(muted);p.drawLine(x+12,ry+14,x+pw-12,ry+14);continue;}
            if(n==panel["selected"].toInt(-1)&&row["enabled"].toBool())p.fillRect(QRect(x+3,ry,pw-6,28),selection);
            p.setFont(app()->font());p.setPen(row["enabled"].toBool()?foreground:muted);
            if(row["checked"].toBool())p.drawText(QRect(x+6,ry,22,28),Qt::AlignCenter,row["radio"].toBool()?QString::fromUtf8("●"):QString::fromUtf8("✓"));
            int reserved=row["shortcut"].toString().isEmpty()?28:164;
            p.drawText(QRect(x+32,ry,pw-32-reserved,28),Qt::AlignVCenter,p.fontMetrics().elidedText(row["text"].toString(),Qt::ElideRight,pw-32-reserved));
            p.drawText(QRect(x+pw-160,ry,132,28),Qt::AlignRight|Qt::AlignVCenter,row["shortcut"].toString());
            if(row["submenu"].toBool())p.drawText(QRect(x+pw-25,ry,20,28),Qt::AlignCenter,QString::fromUtf8("▸"));
        }
        p.restore();
    }
    auto calendar=clock["popup"].toObject();
    if(!calendar.isEmpty()) {
        int x=calendar["x"].toInt(),y=calendar["y"].toInt(),pw=calendar["w"].toInt(),ph=calendar["h"].toInt();
        p.save();p.setClipRect(QRect(x,y,pw,ph));p.fillRect(QRect(x,y,pw,ph),background);p.setPen(foreground);
        p.drawText(QRect(x+8,y+4,36,34),Qt::AlignCenter,"<");p.drawText(QRect(x+48,y+4,pw-96,34),Qt::AlignCenter,calendar["month"].toString());p.drawText(QRect(x+pw-44,y+4,36,34),Qt::AlignCenter,">");
        QStringList days={"Mon","Tue","Wed","Thu","Fri","Sat","Sun"};double cw=(pw-28.)/7.;
        for(int i=0;i<7;++i)p.drawText(QRect(x+14+i*cw,y+44,cw,26),Qt::AlignCenter,days[i]);
        int i=0;for(auto v:calendar["days"].toArray()){auto d=v.toObject();QRect r(x+14+(i%7)*cw,y+72+(i/7)*35,cw,34);++i;if(d["selected"].toBool())p.fillRect(r,selection);p.setPen(d["in_month"].toBool()?foreground:muted);p.drawText(r,Qt::AlignCenter,QString::number(d["day"].toInt()));if(d["today"].toBool())p.fillRect(QRect(r.x()+8,r.bottom()-2,r.width()-16,2),accent);}
        p.setPen(foreground);p.drawText(QRect(x+14,y+290,pw-28,32),Qt::AlignCenter,calendar["available"].toBool()?"Open date in Penguin Mail":"Penguin Mail is not installed");p.drawText(QRect(x+14,y+330,pw-28,30),Qt::AlignCenter,"Time display settings");
        if(calendar["settings_open"].toBool()){int row=0;for(auto v:calendar["options"].toArray()){auto a=v.toArray();QRect r(x+14,y+370+row*32,pw-28,31);if(row==calendar["settings_row"].toInt())p.fillRect(r,selection);p.drawText(r,Qt::AlignLeft|Qt::AlignVCenter,(a[1].toBool()?"[x] ":"[ ] ")+a[0].toString());++row;}}
        p.drawText(QRect(x+14,y+ph-18,pw-28,18),Qt::AlignLeft,calendar["error"].toString());p.restore();
    }
    if(!tray["menu"].isNull()&&!tray["menu"].isUndefined()){auto rows=tray["rows"].toArray();int index=tray["menu"].toInt();int x=qBound(4,tray["keyboard_menu"].toBool()?w-284:w-reserve-(extras.size()-index)*28,qMax(4,w-284));p.fillRect(QRect(x,36,280,rows.size()*28+8),background);for(int n=0;n<rows.size();++n){auto row=rows[n].toObject();QStyleOptionMenuItem item;item.rect=QRect(x+3,40+n*28,274,28);item.text=row["text"].toString();if(row["icon"].toString().startsWith("flag-")){QPixmap flag;flag.loadFromData(QByteArray::fromStdString(dx_flags::svg(row["icon"].toString().mid(5).toStdString(),foreground.name().toStdString())),"SVG");item.icon=QIcon(flag);}item.state=row["enabled"].toBool()?QStyle::State_Enabled:QStyle::State_None;if(n==tray["selected"].toInt(-1))item.state|=QStyle::State_Selected;item.menuItemType=row["separator"].toBool()?QStyleOptionMenuItem::Separator:(row["submenu"].toBool()?QStyleOptionMenuItem::SubMenu:QStyleOptionMenuItem::Normal);item.checked=row["checked"].toBool();item.checkType=row["checkable"].toBool()?QStyleOptionMenuItem::NonExclusive:QStyleOptionMenuItem::NotCheckable;item.palette=menuPalette;if(!row["enabled"].toBool())item.palette.setCurrentColorGroup(QPalette::Disabled);app()->style()->drawControl(QStyle::CE_MenuItem,&item,&p);}}
    auto xrSettings=o["xr_settings"].toObject();
    if(!xrSettings.isEmpty()){
        int x=xrSettings["x"].toInt(),y=xrSettings["y"].toInt(),sw=xrSettings["w"].toInt(),sh=xrSettings["h"].toInt();
        double rowHeight=xrSettings["row_height"].toDouble();
        p.fillRect(QRect(x,y,sw,sh),background);p.setPen(foreground);
        p.drawText(QRect(x+20,y+4,sw-40,32),Qt::AlignVCenter,"Displays, XR & Keybindings");
        auto rows=xrSettings["rows"].toArray();
        int first=xrSettings["first"].toInt(),visible=xrSettings["visible"].toInt();
        p.save();p.setClipRect(QRect(x+12,y+40,sw-24,qRound(visible*rowHeight)));
        for(int n=first;n<qMin(rows.size(),first+visible);++n){auto row=rows[n].toObject();int ry=qRound(y+40+(n-first)*rowHeight),rh=qRound(rowHeight);
            if(n==xrSettings["selected"].toInt())p.fillRect(QRect(x+12,ry,sw-24,rh),selection);
            p.setPen(foreground);p.drawText(QRect(x+20,ry,sw-310,rh),Qt::AlignVCenter,p.fontMetrics().elidedText(row["label"].toString(),Qt::ElideMiddle,sw-310));
            p.drawText(QRect(x+sw-200,ry,140,rh),Qt::AlignCenter,row["value"].toString());
            p.setPen(accent);p.drawText(QRect(x+sw-270,ry,70,rh),Qt::AlignCenter,"−");p.drawText(QRect(x+sw-60,ry,44,rh),Qt::AlignCenter,"+");
        }
        p.restore();
        int sy=y+40+qRound(visible*rowHeight);
        p.setPen(first>0?accent:muted);p.drawText(QRect(x+20,sy,sw/2-20,28),Qt::AlignCenter,first>0?"▲ Scroll up":"▲ Top");
        p.setPen(first+visible<rows.size()?accent:muted);p.drawText(QRect(x+sw/2,sy,sw/2-20,28),Qt::AlignCenter,first+visible<rows.size()?"Scroll down ▼":"Bottom ▼");
        for(auto button:{QRect(x+20,y+sh-66,240,36),QRect(x+280,y+sh-66,240,36),QRect(x+sw-140,y+sh-66,120,36)})p.fillRect(button,selection);
        p.setPen(foreground);p.drawText(QRect(x+20,y+sh-66,240,36),Qt::AlignCenter,"Anchor at headset");p.drawText(QRect(x+280,y+sh-66,240,36),Qt::AlignCenter,"Restore defaults");p.drawText(QRect(x+sw-140,y+sh-66,120,36),Qt::AlignCenter,"Close");
        p.setPen(muted);auto message=xrSettings["error"].toString();if(message.isEmpty())message="Changes save automatically · Arrows select/adjust · Esc closes";
        p.drawText(QRect(x+20,y+sh-28,sw-40,24),Qt::AlignCenter,p.fontMetrics().elidedText(message,Qt::ElideRight,sw-40));
    }
    if(o["about"].toBool()){QRect box((w-440)/2,(h-200)/2,440,200);p.fillRect(box,background);p.setPen(foreground);auto info=o["app_info"].toObject();QString text=o["about_is_app"].toBool()?info["name"].toString()+"\n"+info["comment"].toString()+"\n"+info["desktop_id"].toString():"Dome\nExperimental Rust / Smithay compositor\nQt6 desktop chrome · Native OpenXR";p.drawText(box.adjusted(24,24,-24,-24),Qt::AlignCenter|Qt::TextWordWrap,text+"\n\nClick or press a key to close");}
    if(o["appearance_open"].toBool()&&o["keyboard_settings"].isNull()){
        auto fontChoice=o["appearance_font"].toObject();bool scripts=fontChoice["has_scripts"].toBool();int panelHeight=560+(scripts?48:0);
        QRect box((w-600)/2,(h-panelHeight)/2,600,panelHeight);p.fillRect(box,background);p.setPen(foreground);
        p.drawText(box.adjusted(24,18,-24,-440),Qt::AlignLeft|Qt::AlignVCenter,"Appearance & Input");
        int opacity=barBackground.alpha();
        QString opacityPreset=QString("%1%").arg(qRound(opacity*100./255));
        QString fontName="No Unicode bitmap fonts available",fontSize="Unavailable";
        QString chosen=appearance["font"].toString("Unifont / Sans-Serif");
        for(int i=0;i<bitmapFonts.size();++i){auto f=bitmapFonts[i].toObject();if(f["id"].toString()!=chosen)continue;
            QString name=f["family"].toString();auto style=f["style"].toString();if(style!="Regular"&&style!="Sans-Serif")name+=" · "+style;
            fontName=p.fontMetrics().elidedText(name,Qt::ElideMiddle,200)+QString(" (%1/%2)").arg(i+1).arg(bitmapFonts.size());
            auto sizes=f["sizes"].toArray();int size=app()->font().pixelSize(),sizeIndex=0;for(int n=0;n<sizes.size();++n)if(sizes[n].toInt()==size)sizeIndex=n;
            fontSize=QString("%1 px · native (%2/%3)").arg(size).arg(sizeIndex+1).arg(sizes.size());break;}
        if(!bitmapFonts.isEmpty()&&fontSize=="Unavailable"){fontName="Saved font unavailable";fontSize=QString("%1 px · fallback").arg(app()->font().pixelSize());}
        QStringList names={"Colour scheme","Accent colour","Chrome opacity","Font family · BDF only","Font face","Native size","Font scale","Reset settings"};
        QStringList values={light?"Light":"Dark",accent.name(),opacityPreset,chosen.section(" / ",0,0),chosen.section(" / ",1),fontSize,QString::number(appearance["font_scale"].toInt(1))+"x","Reset"};
        if(!fontChoice.isEmpty()){values[3]=fontChoice["family"].toString();values[4]=fontChoice["face"].toString();values[5]=fontChoice["size"].toString();}
        if(scripts){names.insert(7,"Font script");values.insert(7,fontChoice["script"].toString());}
        for(int n=0;n<names.size();++n){int y=box.y()+60+n*48;if(n==o["shell_selection"].toInt()){p.setPen(accent);p.drawRect(QRect(box.x()+12,y-2,576,44));}p.setPen(foreground);p.drawText(QRect(box.x()+24,y,270,40),Qt::AlignVCenter,p.fontMetrics().elidedText(names[n],Qt::ElideRight,270));p.fillRect(QRect(box.x()+300,y,276,40),selection);p.drawText(QRect(box.x()+308,y,260,40),Qt::AlignCenter,p.fontMetrics().elidedText(values[n],Qt::ElideRight,260));}
        p.setPen(muted);p.drawText(QRect(box.x()+24,box.y()+panelHeight-92,552,28),Qt::AlignCenter,"↑/↓ select · ←/→ adjust · Enter activates");
        p.fillRect(QRect(box.x()+24,box.y()+panelHeight-48,260,36),selection);p.fillRect(QRect(box.x()+316,box.y()+panelHeight-48,260,36),selection);
        if(o["shell_selection"].toInt()>=names.size()){p.setPen(accent);p.drawRect(QRect(box.x()+(o["shell_selection"].toInt()==names.size()?24:316),box.y()+panelHeight-48,260,36).adjusted(0,0,-1,-1));}p.setPen(foreground);p.drawText(QRect(box.x()+24,box.y()+panelHeight-48,260,36),Qt::AlignCenter,"Default applications…");p.drawText(QRect(box.x()+316,box.y()+panelHeight-48,260,36),Qt::AlignCenter,"Close · Esc");
    }
    if(o["appearance_open"].toBool()&&o["keyboard_settings"].isObject()){
        auto k=o["keyboard_settings"].toObject();QRect box((w-600)/2,(h-400)/2,600,400);p.fillRect(box,background);p.setPen(foreground);p.drawText(box.adjusted(24,18,-24,-340),Qt::AlignVCenter,"Keyboard Layouts · desktop and XR");
        QStringList names={"Layouts","Variants","XKB options","Switch shortcut"};QStringList values={k["layout"].toString(),k["variant"].toString(),k["options"].toString(),o["keyboard_shortcut"].toString()};
        for(int i=0;i<4;++i){int y=box.y()+60+i*48;p.setPen(foreground);p.drawText(QRect(box.x()+24,y,156,40),Qt::AlignVCenter,names[i]);QRect field(box.x()+180,y,396,40);p.fillRect(field,selection);if(i==o["keyboard_field"].toInt()){p.setPen(accent);p.drawRect(field.adjusted(0,0,-1,-1));}p.setPen(foreground);p.drawText(field.adjusted(8,0,-8,0),Qt::AlignVCenter,p.fontMetrics().elidedText(values[i],Qt::ElideLeft,380));}
        if(o["keyboard_field"].toInt()>=4){int field=o["keyboard_field"].toInt();p.setPen(accent);p.drawRect(field==4?QRect(box.x()+24,box.y()+252,552,36):QRect(box.x()+(field==5?24:324),box.y()+344,252,40));}p.setPen(foreground);p.drawText(QRect(box.x()+24,box.y()+252,552,36),Qt::AlignVCenter,k["ime"].toBool()?"US / Anthy / Czech / Hangul / Serbian Cyrillic · click for XKB":"Switch XKB groups (click to use input methods)");p.setPen(muted);p.drawText(QRect(box.x()+24,box.y()+290,552,28),Qt::AlignVCenter,"Click field to edit · Ctrl+A clears · ↑/↓ select · ←/→ adjust · Enter applies");p.setPen(accent);p.drawText(QRect(box.x()+24,box.y()+316,552,28),Qt::AlignVCenter,p.fontMetrics().elidedText(o["keyboard_error"].toString(),Qt::ElideRight,552));p.setPen(foreground);p.drawText(QRect(box.x()+24,box.y()+344,252,40),Qt::AlignCenter,"Apply · Enter");p.drawText(QRect(box.x()+324,box.y()+344,252,40),Qt::AlignCenter,"Cancel · Esc");
    }
    if(!o["session_confirm"].isNull()&&!o["session_confirm"].isUndefined()){
        QRect box((w-440)/2,(h-200)/2,440,200);p.fillRect(box,background);p.setPen(foreground);
        auto label=o["session_confirm"].toString();p.drawText(box.adjusted(24,20,-24,-110),Qt::AlignCenter,label+"?");
        p.setPen(muted);p.drawText(box.adjusted(24,70,-24,-60),Qt::AlignCenter|Qt::TextWordWrap,label=="Log Out of Dome"?"Close this Dome compositor session and its application connections.":"This action applies to the computer session.");
        p.fillRect(QRect(box.x()+24,box.y()+140,180,40),selection);p.fillRect(QRect(box.x()+230,box.y()+140,186,40),selection);p.setPen(accent);p.drawRect(QRect(box.x()+(o["shell_selection"].toInt()==1?230:24),box.y()+140,o["shell_selection"].toInt()==1?186:180,40).adjusted(0,0,-1,-1));p.setPen(foreground);p.drawText(QRect(box.x()+24,box.y()+140,180,40),Qt::AlignCenter,"Cancel");p.drawText(QRect(box.x()+230,box.y()+140,186,40),Qt::AlignCenter,label);
    }
    auto defaults=o["default_apps"].toObject();
    if(!defaults.isEmpty()) {
        int x=defaults["x"].toInt(),y=defaults["y"].toInt(),dw=defaults["w"].toInt(),dh=defaults["h"].toInt();
        bool chooser=defaults["chooser"].toBool(),busy=defaults["busy"].toBool();auto rows=defaults["rows"].toArray();
        p.fillRect(QRect(x,y,dw,dh),background);p.setPen(foreground);p.drawText(QRect(x+20,y+8,dw-40,30),Qt::AlignVCenter,p.fontMetrics().elidedText(defaults["title"].toString(),Qt::ElideRight,dw-40));
        p.setPen(muted);p.drawText(QRect(x+20,y+36,dw-40,24),Qt::AlignVCenter,chooser?"Select an application, then use the button below":"Default handlers for this Dome session");
        int first=defaults["first"].toInt(),visible=defaults["visible"].toInt();
        p.save();p.setClipRect(QRect(x+12,y+64,dw-24,visible*44));
        for(int i=first;i<qMin(rows.size(),first+visible);++i){auto row=rows[i].toObject();int ry=y+64+(i-first)*44;
            if(i==defaults["selected"].toInt())p.fillRect(QRect(x+12,ry,dw-24,42),selection);
            p.setPen(foreground);
            if(chooser){p.drawText(QRect(x+20,ry,dw-40,22),Qt::AlignVCenter,p.fontMetrics().elidedText((row["current"].toBool()?"● ":"○ ")+row["label"].toString(),Qt::ElideRight,dw-40));p.setPen(muted);p.drawText(QRect(x+44,ry+22,dw-64,20),Qt::AlignVCenter,p.fontMetrics().elidedText(row["detail"].toString(),Qt::ElideMiddle,dw-64));}
            else {p.drawText(QRect(x+20,ry,dw*2/5-30,42),Qt::AlignVCenter,p.fontMetrics().elidedText(row["label"].toString(),Qt::ElideRight,dw*2/5-30));p.setPen(row["available"].toBool()?foreground:muted);p.drawText(QRect(x+dw*2/5,ry,dw*3/5-32,42),Qt::AlignVCenter,p.fontMetrics().elidedText(row["detail"].toString()+(row["available"].toBool()?"  ›":" · no handlers installed"),Qt::ElideRight,dw*3/5-32));}
        }p.restore();
        QString message=defaults["error"].toString();if(message.isEmpty())message=busy?"Updating applications…":QString("%1–%2 of %3 · Arrows select · Enter opens/applies · Esc goes back").arg(rows.isEmpty()?0:first+1).arg(qMin(rows.size(),first+visible)).arg(rows.size());
        p.setPen(defaults["error"].toString().isEmpty()?muted:accent);p.drawText(QRect(x+20,y+dh-90,dw-40,30),Qt::AlignCenter,p.fontMetrics().elidedText(message,Qt::ElideRight,dw-40));
        p.fillRect(QRect(x+20,y+dh-54,dw/2-30,36),selection);p.fillRect(QRect(x+dw/2+10,y+dh-54,dw/2-30,36),selection);p.setPen(foreground);
        p.drawText(QRect(x+20,y+dh-54,dw/2-30,36),Qt::AlignCenter,chooser?"Back":"Refresh");p.drawText(QRect(x+dw/2+10,y+dh-54,dw/2-30,36),Qt::AlignCenter,chooser?"Use selected application":"Close");
    }
    auto audio=o["audio"].toObject();
    if(!audio.isEmpty()) {
        int x=audio["x"].toInt(),y=audio["y"].toInt(),aw=audio["w"].toInt(),ah=audio["h"].toInt(),selected=audio["selected"].toInt();
        p.save();p.setClipRect(QRect(x,y,aw,ah));p.fillRect(QRect(x,y,aw,ah),background);p.setPen(foreground);p.drawText(QRect(x+16,y+4,aw-60,32),Qt::AlignVCenter,"Audio");p.drawText(QRect(x+aw-36,y,36,36),Qt::AlignCenter,"×");
        for(int index=0;index<2;++index){auto level=audio[index==0?"output":"input"].toObject();bool available=!level.isEmpty(),mutedLevel=level["muted"].toBool();double volume=level["volume"].toDouble();int offset=index==0?64:136;
            if(selected==index)p.fillRect(QRect(x+10,y+offset-30,aw-20,64),selection);
            p.setPen(available?foreground:muted);QString label=index==0?"Output":"Microphone";QString percentage=available?QString::number(qRound(volume*100))+"%":"Unavailable";
            QString name=audio[index==0?"output_name":"input_name"].toString();p.drawText(QRect(x+16,y+offset-28,aw-32,24),Qt::AlignVCenter,p.fontMetrics().elidedText(label+"  "+percentage+" · "+name,Qt::ElideRight,aw-32));
            p.fillRect(QRect(x+16,y+offset,32,30),barBackground);p.fillRect(QRect(x+aw-140,y+offset,32,30),barBackground);p.fillRect(QRect(x+aw-96,y+offset,80,30),barBackground);
            p.drawText(QRect(x+16,y+offset,32,30),Qt::AlignCenter,"−");p.drawText(QRect(x+aw-140,y+offset,32,30),Qt::AlignCenter,"+");p.drawText(QRect(x+aw-96,y+offset,80,30),Qt::AlignCenter,mutedLevel?"Unmute":"Mute");
            QRect track(x+60,y+offset+11,aw-216,8);p.fillRect(track,muted);if(available){p.fillRect(QRect(track.x(),track.y(),qRound(qBound(0.,volume,1.)*track.width()),track.height()),mutedLevel?muted:accent);p.setPen(available?foreground:muted);int thumb=track.x()+qRound(qBound(0.,volume,1.)*track.width());p.drawLine(thumb,track.y()-5,thumb,track.y()+12);}
        }
        p.setPen(muted);p.drawText(QRect(x+16,y+182,aw-32,28),Qt::AlignVCenter,"Default devices");auto rows=audio["rows"].toArray();int first=audio["first"].toInt(),visible=audio["visible"].toInt();
        for(int index=first;index<qMin(rows.size(),first+visible);++index){auto device=rows[index].toObject();int ry=y+214+(index-first)*32;if(selected==index+2)p.fillRect(QRect(x+10,ry,aw-20,30),selection);p.setPen(device["active"].toBool()?accent:foreground);p.drawText(QRect(x+16,ry,22,30),Qt::AlignCenter,device["active"].toBool()?"●":"○");QString label=(device["target"].toString()=="output"?"Output · ":"Microphone · ")+device["label"].toString();p.drawText(QRect(x+44,ry,aw-60,30),Qt::AlignVCenter,p.fontMetrics().elidedText(label,Qt::ElideRight,aw-60));}
        auto error=audio["error"].toString();p.setPen(error.isEmpty()?muted:accent);p.drawText(QRect(x+16,y+ah-30,aw-32,24),Qt::AlignCenter,p.fontMetrics().elidedText(error.isEmpty()?"Arrows select/adjust · Enter mutes or selects · Esc closes":error,Qt::ElideRight,aw-32));p.restore();
    }
    auto history=o["history"].toObject();
    if(!history.isEmpty()) {
        int x=history["x"].toInt(),y=history["y"].toInt(),hw=history["w"].toInt(),hh=history["h"].toInt();
        p.fillRect(QRect(x,y,hw,hh),background);p.setPen(foreground);p.drawText(QRect(x+18,y+4,hw-36,32),Qt::AlignVCenter,history["title"].toString());
        auto rows=history["rows"].toArray();int first=history["first"].toInt(),visible=history["visible"].toInt();
        p.save();p.setClipRect(QRect(x+10,y+44,hw-20,visible*64));
        for(int index=first;index<qMin(rows.size(),first+visible);++index){int ry=y+44+(index-first)*64;auto row=rows[index].toObject();
            if(index==history["selected"].toInt())p.fillRect(QRect(x+10,ry,hw-20,62),selection);
            p.setPen(muted);auto date=QDateTime::fromSecsSinceEpoch(qint64(row["timestamp"].toDouble())).toString("ddd HH:mm");p.drawText(QRect(x+18,ry+2,hw-150,18),Qt::AlignVCenter,p.fontMetrics().elidedText(row["detail"].toString(),Qt::ElideRight,hw-150));p.drawText(QRect(x+hw-130,ry+2,92,18),Qt::AlignRight|Qt::AlignVCenter,date);
            p.setPen(foreground);p.drawText(QRect(x+18,ry+22,hw-66,38),Qt::AlignLeft|Qt::AlignTop|Qt::TextWordWrap,p.fontMetrics().elidedText(row["text"].toString(),Qt::ElideRight,(hw-66)*2));p.drawText(QRect(x+hw-36,ry,26,62),Qt::AlignCenter,"×");
            auto image=row["image"].toObject();int iw=image["width"].toInt(),ih=image["height"].toInt();
            auto pixels=QByteArray::fromBase64(image["rgba"].toString().toLatin1());
            if(iw>0&&ih>0&&iw<=1024&&ih<=512&&pixels.size()==qint64(iw)*ih*4) {
                QImage content(reinterpret_cast<const uchar*>(pixels.constData()),iw,ih,QImage::Format_RGBA8888);
                auto preview=content.scaled(240,38,Qt::KeepAspectRatio,Qt::SmoothTransformation);
                p.drawImage(x+hw-50-preview.width(),ry+22,preview);
            }

        }
        if(rows.isEmpty()){p.setPen(muted);p.drawText(QRect(x+18,y+60,hw-36,80),Qt::AlignCenter,"History is empty");}p.restore();
        p.setPen(muted);p.drawText(QRect(x+18,y+hh-60,hw-36,18),Qt::AlignCenter,history["kind"].toString()=="clipboard"?"Select an item to copy · Arrows browse · Delete removes":"Arrows browse · Delete removes · Escape closes");
        p.fillRect(QRect(x+14,y+hh-36,hw/2-21,30),selection);p.fillRect(QRect(x+hw/2+7,y+hh-36,hw/2-21,30),selection);p.setPen(foreground);p.drawText(QRect(x+14,y+hh-36,hw/2-21,30),Qt::AlignCenter,"Clear history");p.drawText(QRect(x+hw/2+7,y+hh-36,hw/2-21,30),Qt::AlignCenter,"Close");
    }
    auto error=o["session_error"].toString();if(!error.isEmpty()){p.fillRect(QRect(w/2-300,h-106,600,40),background);p.setPen(foreground);p.drawText(QRect(w/2-290,h-106,580,40),Qt::AlignVCenter,p.fontMetrics().elidedText(error,Qt::ElideRight,580));}
    }
    for(auto value:o["notifications"].toArray()){
        auto n=value.toObject();int x=n["x"].toInt(),y=n["y"].toInt(),nw=n["w"].toInt(),nh=n["h"].toInt();
        p.save();p.setClipRect(QRect(x,y,nw,nh));p.fillRect(QRect(x,y,nw,nh),barBackground);
        p.fillRect(QRect(x,y,3,nh),n["critical"].toBool()?QColor(240,105,80):accent);
        p.setPen(muted);p.drawText(QRect(x+12,y+4,nw-52,22),Qt::AlignVCenter,p.fontMetrics().elidedText(n["app"].toString(),Qt::ElideRight,nw-52));
        p.setPen(foreground);p.drawText(QRect(x+nw-32,y,32,32),Qt::AlignCenter,"×");
        auto noticeImage=n["image"].toObject();
        if(!noticeImage.isEmpty()) {
            auto bytes=QByteArray::fromBase64(noticeImage["rgba"].toString().toLatin1());
            int iw=noticeImage["width"].toInt(),ih=noticeImage["height"].toInt();
            if(iw>0&&ih>0&&iw<=1024&&ih<=512&&bytes.size()==qint64(iw)*ih*4) {
                QImage content(reinterpret_cast<const uchar*>(bytes.constData()),iw,ih,QImage::Format_RGBA8888);
                p.drawImage(QRect(x+8,y+32,nw-16,nh-36),content);
            }
            p.restore();continue;
        }
        auto bold=p.font();bold.setBold(true);p.setFont(bold);p.drawText(QRect(x+12,y+28,nw-24,24),Qt::AlignVCenter,p.fontMetrics().elidedText(n["summary"].toString(),Qt::ElideRight,nw-24));p.setFont(app()->font());
        p.save();p.setClipRect(QRect(x+12,y+54,nw-24,42));p.drawText(QRect(x+12,y+54,nw-24,42),Qt::AlignLeft|Qt::AlignTop|Qt::TextWordWrap,n["body"].toString());p.restore();
        int ai=0;for(auto action:n["actions"].toArray()) {auto a=action.toArray();if(a[0].toString()=="default")continue;QRect button(x+6+(ai%2)*(nw/2),y+100+(ai/2)*32,nw/2-12,28);p.fillRect(button,selection);p.drawText(button.adjusted(4,0,-4,0),Qt::AlignCenter,p.fontMetrics().elidedText(a[1].toString(),Qt::ElideRight,button.width()-8));++ai;}
        p.restore();
    }
    return 1;
}

#include "bdf_font_ffi.hpp"
