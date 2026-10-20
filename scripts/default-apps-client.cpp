// Backend fixture: no connection to the user's compositor or D-Bus session.
#include <iostream>
extern "C" const char *dome_default_apps_list();
extern "C" const char *dome_default_app_set(const char*,const char*);
int main(int argc,char** argv){
    if(argc==2&&std::string(argv[1])=="list"){std::cout<<dome_default_apps_list()<<'\n';return 0;}
    if(argc==4&&std::string(argv[1])=="set"){auto error=dome_default_app_set(argv[2],argv[3]);if(*error){std::cerr<<error<<'\n';return 1;}return 0;}
    return 2;
}
