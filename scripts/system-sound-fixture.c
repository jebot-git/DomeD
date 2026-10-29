/* Private libcanberra test double. Used only through the fixture's LD_LIBRARY_PATH. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <unistd.h>
typedef struct { char event[128]; } Properties;
static void record(const char *kind, const char *event) {
    const char *path = getenv("DOME_SOUND_TEST_RECORD");
    if (!path) return;
    FILE *f = fopen(path,"a");
    if (f) { fprintf(f,"%s %s\n",kind,event ? event : ""); fclose(f); }
}
int ca_context_create(void **p) { if (getenv("DOME_SOUND_FIXTURE_FAIL")) return -1; *p=calloc(1,1);return *p ? 0 : -1; }
int ca_context_destroy(void *p) { free(p);return 0; }
int ca_proplist_create(void **p) { *p=calloc(1,sizeof(Properties));return *p ? 0 : -1; }
int ca_proplist_destroy(void *p) { free(p);return 0; }
int ca_proplist_sets(void *p,const char *key,const char *value) { if (!strcmp(key,"event.id")) snprintf(((Properties*)p)->event,128,"%s",value); return 0; }
int ca_context_change_props_full(void *ctx,void *props) { (void)ctx;(void)props;return 0; }
int ca_context_cancel(void *ctx,uint32_t id) { (void)ctx;(void)id;record("cancel",NULL);return 0; }
int ca_context_play_full(void *ctx,uint32_t id,void *props,void (*cb)(void*,uint32_t,int,void*),void *data) {
    (void)ctx;(void)id;(void)cb;(void)data;record("play",((Properties*)props)->event);
    if (getenv("DOME_SOUND_FIXTURE_DELAY")) usleep(1000000);
    return 0;
}
