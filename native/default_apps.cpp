// SPDX-License-Identifier: MIT
#include <gio/gdesktopappinfo.h>
#include <QtCore/QJsonArray>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonObject>
#include <QtCore/QDir>
#include <QtCore/QFileInfo>
#include <QtCore/QSaveFile>
#include <algorithm>
#include <QtCore/QSet>

struct FileType {const char *mime,*label;};
static const FileType types[]={
    {"inode/directory","Folders"},
    {"text/plain","Text (.txt)"},{"text/markdown","Markdown (.md)"},
    {"application/pdf","PDF documents"},{"text/html","HTML documents"},
    {"application/json","JSON files"},
    {"text/csv","CSV tables"},
    {"application/vnd.openxmlformats-officedocument.wordprocessingml.document","Word documents (.docx)"},
    {"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet","Excel spreadsheets (.xlsx)"},
    {"application/vnd.openxmlformats-officedocument.presentationml.presentation","PowerPoint slides (.pptx)"},
    {"application/vnd.oasis.opendocument.text","OpenDocument text (.odt)"},
    {"application/vnd.oasis.opendocument.spreadsheet","OpenDocument spreadsheets (.ods)"},
    {"application/vnd.oasis.opendocument.presentation","OpenDocument slides (.odp)"},
    {"application/msword","Word documents (.doc)"},
    {"application/vnd.ms-excel","Excel spreadsheets (.xls)"},
    {"application/vnd.ms-powerpoint","PowerPoint slides (.ppt)"},
    {"image/png","PNG images"},{"image/jpeg","JPEG images"},{"image/gif","GIF images"},
    {"image/webp","WebP images"},{"image/svg+xml","SVG images"},
    {"audio/mpeg","MP3 audio"},{"audio/flac","FLAC audio"},{"audio/ogg","Ogg audio"},
    {"video/mp4","MP4 video"},{"video/webm","WebM video"},{"video/x-matroska","Matroska video"},
    {"application/zip","ZIP archives"},{"application/x-7z-compressed","7-Zip archives"},
    {"application/x-compressed-tar","Compressed tar archives"},
    {"x-scheme-handler/https","Web links (https)"},{"x-scheme-handler/http","Web links (http)"},
    {"x-scheme-handler/mailto","Email links"},
};
static QString configPath(){auto root=qEnvironmentVariable("XDG_CONFIG_HOME");return (root.isEmpty()?QDir::homePath()+"/.config":root)+"/dome-mimeapps.list";}
static GKeyFile *preferences(QString &message){
    auto file=g_key_file_new();GError *error=nullptr;
    if(!g_key_file_load_from_file(file,configPath().toUtf8().constData(),GKeyFileFlags(G_KEY_FILE_KEEP_COMMENTS|G_KEY_FILE_KEEP_TRANSLATIONS),&error)){
        if(!g_error_matches(error,G_FILE_ERROR,G_FILE_ERROR_NOENT))message=QString::fromUtf8(error->message);
        g_clear_error(&error);
    }return file;
}
static QJsonObject describe(GAppInfo *app){return {{"id",QString::fromUtf8(g_app_info_get_id(app))},{"name",QString::fromUtf8(g_app_info_get_display_name(app))}};}
static QString executablePath(GAppInfo *app){
    // GIO's get_executable retains Exec quoting. Decode the argument syntax
    // without running a shell, so quoted absolute paths (including spaces) work.
    const char *command=g_app_info_get_commandline(app);gchar **args=nullptr;gint count=0;
    if(!command||!g_shell_parse_argv(command,&count,&args,nullptr)||count<1){g_strfreev(args);return {};}
    auto executable=QString::fromUtf8(args[0]).replace("%%","%");g_strfreev(args);return executable;
}
static bool available(GAppInfo *app){
    if(!g_app_info_get_id(app)||g_desktop_app_info_get_is_hidden(G_DESKTOP_APP_INFO(app)))return false;
    auto executable=executablePath(app).toUtf8();if(executable.isEmpty())return false;
    auto path=g_find_program_in_path(executable.constData());bool ok=path;g_free(path);return ok;
}
static bool supports(GAppInfo *app,const char *mime){
    auto list=g_app_info_get_supported_types(app);if(!list)return false;
    for(auto item=list;*item;++item)if(g_content_type_is_a(mime,*item)||g_str_equal(mime,*item))return true;
    return false;
}
static GAppInfo *scopedDefault(GKeyFile *file,const char *mime){
    auto ids=g_key_file_get_string_list(file,"Default Applications",mime,nullptr,nullptr);
    GAppInfo *result=nullptr;
    if(ids)for(auto id=ids;*id&&!result;++id){auto app=g_desktop_app_info_new(*id);if(app){if(available(G_APP_INFO(app)))result=G_APP_INFO(app);else g_object_unref(app);}}
    g_strfreev(ids);return result;
}
static QString save(GKeyFile *file){
    auto path=configPath();if(!QDir().mkpath(QFileInfo(path).absolutePath()))return "Cannot create the user configuration directory";
    gsize length=0;auto bytes=g_key_file_to_data(file,&length,nullptr);
    QSaveFile output(path);bool ok=output.open(QIODevice::WriteOnly)&&output.write(bytes,length)==qint64(length)&&output.commit();g_free(bytes);
    return ok?QString():output.errorString();
}
static QString seedFileManager(GKeyFile *file){
    // An explicit Dome choice always wins. Other desktop sessions are untouched.
    if(g_key_file_has_key(file,"Default Applications","inode/directory",nullptr))return {};
    auto apps=g_app_info_get_all_for_type("inode/directory");GAppInfo *preferred=nullptr;int rank=100;
    for(auto node=apps;node;node=node->next){auto app=G_APP_INFO(node->data);if(!available(app))continue;
        auto exe=QFileInfo(executablePath(app)).fileName();
        auto aliases=QStringList{"fsvr","fsx-vr","fsv-xr"};int candidate=aliases.indexOf(exe);
        if(candidate>=0&&candidate<rank){rank=candidate;preferred=app;}
    }
    QString error;
    if(preferred){const char *id=g_app_info_get_id(preferred);g_key_file_set_string_list(file,"Default Applications","inode/directory",&id,1);
        if(!g_key_file_has_key(file,"Default Applications","x-scheme-handler/trash",nullptr)&&supports(preferred,"x-scheme-handler/trash"))
            g_key_file_set_string_list(file,"Default Applications","x-scheme-handler/trash",&id,1);
        error=save(file);
    }
    g_list_free_full(apps,g_object_unref);return error;
}
static QString seedPdfViewer(GKeyFile *file){
    // Miro is recommended, not required. An explicit Dome choice (even a
    // removed handler) is never replaced, and no alternative is chosen for it.
    if(g_key_file_has_key(file,"Default Applications","application/pdf",nullptr))return {};
    auto apps=g_app_info_get_all_for_type("application/pdf");GAppInfo *preferred=nullptr;
    for(auto node=apps;node;node=node->next){auto app=G_APP_INFO(node->data);
        if(!available(app)||!supports(app,"application/pdf"))continue;
        auto executable=QFileInfo(executablePath(app)).fileName();
        if(executable=="miro-pdf"){
            preferred=app;
            if(g_str_equal(g_app_info_get_id(app),"miro-pdf.desktop"))break;
        }
    }
    QString error;
    if(preferred){const char *id=g_app_info_get_id(preferred);
        g_key_file_set_string_list(file,"Default Applications","application/pdf",&id,1);error=save(file);}
    g_list_free_full(apps,g_object_unref);return error;
}
extern "C" const char *dome_default_apps_list(){
    thread_local QByteArray result;QString error;auto file=preferences(error);if(error.isEmpty())error=seedFileManager(file);if(error.isEmpty())error=seedPdfViewer(file);QJsonArray rows;
    for(auto type:types){
        QJsonArray choices;auto apps=g_app_info_get_all_for_type(type.mime);QList<QJsonObject> sorted;
        for(auto node=apps;node;node=node->next){auto app=G_APP_INFO(node->data);if(available(app))sorted.append(describe(app));}
        auto current=scopedDefault(file,type.mime);if(!current&&!g_str_equal(type.mime,"application/pdf"))current=g_app_info_get_default_for_type(type.mime,false);
        QJsonValue selected=QJsonValue::Null;
        if(current){selected=describe(current);auto id=g_app_info_get_id(current);bool found=false;
            for(const auto &item:sorted)if(item["id"].toString()==QString::fromUtf8(id)){found=true;break;}
            if(!found&&available(current)){sorted.append(describe(current));}g_object_unref(current);}
        std::sort(sorted.begin(),sorted.end(),[](const auto &a,const auto &b){return QString::localeAwareCompare(a["name"].toString(),b["name"].toString())<0;});
        QSet<QString> seen;for(const auto &item:sorted)if(!seen.contains(item["id"].toString())){seen.insert(item["id"].toString());choices.append(item);}
        rows.append(QJsonObject{{"mime",type.mime},{"label",type.label},{"current",selected},{"apps",choices}});g_list_free_full(apps,g_object_unref);
    }
    g_key_file_unref(file);result=QJsonDocument(QJsonObject{{"rows",rows},{"error",error}}).toJson(QJsonDocument::Compact);return result.constData();
}
extern "C" const char *dome_default_app_set(const char *mime,const char *id){
    thread_local QByteArray error;error.clear();bool known=false;for(auto type:types)if(g_str_equal(mime,type.mime))known=true;
    if(!known){error="Unknown file type";return error.constData();}
    auto app=g_desktop_app_info_new(id);
    if(!app||!available(G_APP_INFO(app))){if(app)g_object_unref(app);error="The selected application is no longer installed";return error.constData();}
    if(!supports(G_APP_INFO(app),mime)){g_object_unref(app);error="The application does not declare support for this file type";return error.constData();}
    QString message;auto file=preferences(message);if(message.isEmpty()){g_key_file_set_string_list(file,"Default Applications",mime,&id,1);message=save(file);}error=message.toUtf8();g_key_file_unref(file);g_object_unref(app);return error.constData();
}
