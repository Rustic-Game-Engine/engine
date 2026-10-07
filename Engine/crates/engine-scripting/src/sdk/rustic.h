#pragma once
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>
#include <ctype.h>
#include <math.h>
#include <stdint.h>
#include <inttypes.h>

/* Engine-owned C17 SDK. Values and strings returned by reads last until the next callback. */
typedef struct { double x,y,z; } RusticVector3;
typedef enum { RUSTIC_NULL, RUSTIC_BOOL, RUSTIC_INTEGER, RUSTIC_NUMBER, RUSTIC_STRING, RUSTIC_VECTOR } RusticType;
typedef struct { RusticType type; bool boolean; int64_t integer; double number; const char *string; double vector[3]; size_t length; } RusticValue;
typedef struct { bool pressed,released,held; double axis; } RusticActionState;
typedef struct { const char *key,*state; bool repeat; } RusticKeyEvent;
typedef struct { size_t count; const char **items; } RusticList;
typedef struct { size_t count; RusticKeyEvent *items; } RusticKeyEvents;
typedef struct {
    void (*on_create)(void), (*on_start)(void), (*on_enable)(void), (*on_disable)(void), (*on_destroy)(void), (*on_stop)(void);
    void (*fixed_update)(double), (*update)(double);
} RusticBehavior;

/* Private transport implementation. */
typedef struct { size_t start,end,next; char kind; char *decoded; } RToken;
static char r_source[1048578], r_output[1048578];
static RToken r_tokens[65536];
static size_t r_count,r_at,r_written,r_commands;
static void r_fail(const char *message) { fputs(message,stderr); fputc('\n',stderr); exit(1); }
static void r_ws(void) { while (isspace((unsigned char)r_source[r_at])) ++r_at; }
static size_t r_parse(unsigned depth) {
    if (depth>64 || r_count>=65536) r_fail("Rustic C: state nesting or token limit");
    r_ws(); size_t i=r_count++; RToken *t=&r_tokens[i]; t->start=r_at; t->kind=r_source[r_at];
    if (t->kind=='"') {
        ++r_at;
        while(r_source[r_at] && r_source[r_at]!='"') { if(r_source[r_at]=='\\') { ++r_at; if(!r_source[r_at]) r_fail("Rustic C: truncated string"); } ++r_at; }
        if(r_source[r_at++]!='"') r_fail("Rustic C: missing quote");
    } else if (t->kind=='{' || t->kind=='[') {
        char end=t->kind=='{'?'}':']'; ++r_at; r_ws();
        while(r_source[r_at]!=end) {
            (void)r_parse(depth+1); r_ws();
            if(r_source[r_at]==':' || r_source[r_at]==',') { ++r_at; r_ws(); }
            else if(r_source[r_at]!=end) r_fail("Rustic C: invalid state");
        }
        ++r_at;
    } else {
        while(r_source[r_at] && !isspace((unsigned char)r_source[r_at]) && !strchr(",:]}",r_source[r_at])) ++r_at;
        if(r_at==t->start) r_fail("Rustic C: invalid token");
    }
    t->end=r_at; t->next=r_count; return i;
}
static unsigned r_hex(const char *p) { unsigned n=0; for(int i=0;i<4;++i) { char c=p[i]; unsigned v=c>='0'&&c<='9'?(unsigned)(c-'0'):c>='a'&&c<='f'?(unsigned)(c-'a'+10):c>='A'&&c<='F'?(unsigned)(c-'A'+10):16; if(v>15)r_fail("Rustic C: invalid Unicode"); n=n*16+v; } return n; }
static const char *r_string(size_t i) {
    if(i>=r_count || r_tokens[i].kind!='"') return NULL;
    RToken *t=&r_tokens[i]; if(t->decoded) return t->decoded;
    char *out=(char*)malloc(t->end-t->start+1); if(!out)r_fail("Rustic C: allocation failed");
    size_t n=0;
    for(size_t j=t->start+1;j<t->end-1;++j) {
        unsigned char c=(unsigned char)r_source[j];
        if(c=='\\') {
            c=(unsigned char)r_source[++j];
            if(c=='u') {
                unsigned v=r_hex(r_source+j+1); j+=4;
                if(v>=0xd800&&v<=0xdbff) { if(r_source[j+1]!='\\'||r_source[j+2]!='u')r_fail("Rustic C: missing surrogate"); unsigned low=r_hex(r_source+j+3); if(low<0xdc00||low>0xdfff)r_fail("Rustic C: invalid surrogate"); v=0x10000+((v-0xd800)<<10)+(low-0xdc00); j+=6; }
                if(v==0)r_fail("Rustic C: embedded NUL is unsupported");
                if(v<0x80)out[n++]=(char)v;
                else if(v<0x800) {out[n++]=(char)(0xc0|(v>>6));out[n++]=(char)(0x80|(v&63));}
                else if(v<0x10000) {out[n++]=(char)(0xe0|(v>>12));out[n++]=(char)(0x80|((v>>6)&63));out[n++]=(char)(0x80|(v&63));}
                else {out[n++]=(char)(0xf0|(v>>18));out[n++]=(char)(0x80|((v>>12)&63));out[n++]=(char)(0x80|((v>>6)&63));out[n++]=(char)(0x80|(v&63));}
                continue;
            }
            switch(c) {case 'n':c='\n';break;case 'r':c='\r';break;case 't':c='\t';break;case 'b':c='\b';break;case 'f':c='\f';break;default:break;}
        }
        out[n++]=(char)c;
    }
    out[n]=0; t->decoded=out; return out;
}
static size_t r_field(size_t object,const char *name) {
    if(object>=r_count || r_tokens[object].kind!='{') return r_count;
    for(size_t i=object+1;i<r_tokens[object].next;) { const char *key=r_string(i); size_t v=i+1; if(key&&strcmp(key,name)==0)return v; i=r_tokens[v].next; }
    return r_count;
}
static double r_number(size_t i) { return i<r_count ? strtod(r_source+r_tokens[i].start,NULL):0; }
static bool r_bool(size_t i) { return i<r_count && r_tokens[i].kind=='t'; }
static RusticValue r_value(size_t i) {
    RusticValue v={0}; if(i>=r_count)return v;
    char k=r_tokens[i].kind;
    if(k=='"') {v.type=RUSTIC_STRING;v.string=r_string(i);}
    else if(k=='t'||k=='f') {v.type=RUSTIC_BOOL;v.boolean=k=='t';}
    else if(k=='[') {v.type=RUSTIC_VECTOR;for(size_t j=i+1;j<r_tokens[i].next&&v.length<3;j=r_tokens[j].next)v.vector[v.length++]=r_number(j);}
    else if(k!='n') {v.type=RUSTIC_NUMBER;v.number=r_number(i);
        bool integral=true;for(size_t j=r_tokens[i].start;j<r_tokens[i].end;++j)if(strchr(".eE",r_source[j]))integral=false;
        if(integral){v.type=RUSTIC_INTEGER;v.integer=(int64_t)strtoll(r_source+r_tokens[i].start,NULL,10);}
    }
    return v;
}
static void r_append(const char *s) { size_t n=strlen(s); if(r_written+n>=sizeof r_output)r_fail("Rustic C: command limit"); memcpy(r_output+r_written,s,n+1);r_written+=n; }
static void r_quote(const char *s) {
    if(!s) {r_append("null");return;} r_append("\"");
    for(const unsigned char *p=(const unsigned char*)s;*p;++p) { char b[8]; if(*p<32||*p=='"'||*p=='\\')snprintf(b,sizeof b,"\\u%04x",*p);else {b[0]=(char)*p;b[1]=0;} r_append(b); } r_append("\"");
}
static void r_double(double x) {char b[64];if(!isfinite(x))r_fail("Rustic C: non-finite value");snprintf(b,sizeof b,"%.17g",x);r_append(b);}
static void r_write_value(RusticValue v) {switch(v.type){case RUSTIC_BOOL:r_append(v.boolean?"true":"false");break;case RUSTIC_INTEGER:{char b[64];snprintf(b,sizeof b,"%" PRId64,v.integer);r_append(b);break;}case RUSTIC_NUMBER:r_double(v.number);break;case RUSTIC_STRING:r_quote(v.string);break;case RUSTIC_VECTOR:r_append("[");if(v.length>3)r_fail("Rustic C: invalid vector");for(size_t i=0;i<v.length;++i){if(i)r_append(",");r_double(v.vector[i]);}r_append("]");break;default:r_append("null");}}
static void r_command(const char *op) {if(r_commands++)r_append(",");r_append("{\"op\":");r_quote(op);}
static const char *r_entity_id(void) {return r_string(r_field(0,"entity_id"));}
static double r_delta_time(void) {return r_number(r_field(0,"delta_time"));}
static double r_fixed_delta_time(void) {return r_number(r_field(0,"fixed_delta_time"));}
static RusticVector3 r_translation(void) {RusticValue v=r_value(r_field(0,"translation"));return (RusticVector3){v.vector[0],v.vector[1],v.vector[2]};}
static void r_set_translation(double x,double y,double z) {r_command("set_translation");r_append(",\"value\":[");r_double(x);r_append(",");r_double(y);r_append(",");r_double(z);r_append("]}");}
static RusticValue r_property(const char *n) {return r_value(r_field(r_field(0,"properties"),n));}
static RusticValue r_attribute(const char *n) {return r_value(r_field(r_field(0,"attributes"),n));}
static void r_named(const char *op,const char *name,RusticValue value) {r_command(op);r_append(",\"name\":");r_quote(name);r_append(",\"value\":");r_write_value(value);r_append("}");}
static void r_set_property(const char *n,RusticValue v) {r_named("set_property",n,v);}
static void r_edit_attribute(const char *n,RusticValue v) {r_named("edit_attribute",n,v);}
static void r_edit_object_attribute(const char *scene,const char *path,const char *name,RusticValue value) {
    char source[4096];
    int n=snprintf(source,sizeof source,"rustic.game.%s.%s",scene,path);
    if(n<0 || (size_t)n>=sizeof source)r_fail("Rustic C: object path limit");
    r_command("edit_attribute");r_append(",\"source\":");r_quote(source);
    r_append(",\"name\":");r_quote(name);r_append(",\"value\":");r_write_value(value);r_append("}");
}
static RusticActionState r_action(const char *group,const char *n) {size_t i=r_field(r_field(0,group),n);return (RusticActionState){r_bool(r_field(i,"pressed")),r_bool(r_field(i,"released")),r_bool(r_field(i,"held")),r_number(r_field(i,"axis"))};}
static RusticActionState r_input(const char *n) {return r_action("actions",n);}
static RusticActionState r_key(const char *n) {return r_action("keys",n);}
static bool r_any(void) {return r_bool(r_field(0,"any_key_pressed"));}
static void r_log(const char *l,const char *m) {r_command("log");r_append(",\"level\":");r_quote(l);r_append(",\"message\":");r_quote(m);r_append("}");}
static void r_enabled(bool e) {r_command("set_enabled");r_append(e?",\"enabled\":true}":",\"enabled\":false}");}
static void r_instance(const char *op,const char *s,const char *p) {r_command(op);r_append(",\"source\":");r_quote(s);r_append(",\"parent\":");r_quote(p);r_append("}");}
static void r_add(const char *s,const char *p) {r_instance("add_instance",s,p);}
static void r_clone(const char *s,const char *p) {r_instance("clone_instance",s,p);}
static void r_camera(const char *s) {r_command("set_current_camera");r_append(",\"source\":");r_quote(s);r_append("}");}
static const char *r_find(const char *s) {return r_string(r_field(r_field(0,"scene_paths"),s));}
static RusticList r_list(const char *path) {
    static const char *items[65536]; RusticList out={0,items}; size_t o=r_field(0,"scene_paths"); if(o>=r_count)return out;
    size_t n=path?strlen(path):0;bool all=!path||!*path||strcmp(path,"Game.scene")==0;
    for(size_t i=o+1;i<r_tokens[o].next;i=r_tokens[i+1].next) {const char *name=r_string(i);if(all||(strncmp(name,path,n)==0&&name[n]=='.'))items[out.count++]=r_string(i+1);}
    return out;
}
static RusticKeyEvents r_events(void) {
    static RusticKeyEvent items[65536];RusticKeyEvents out={0,items};size_t a=r_field(0,"key_events");if(a>=r_count)return out;
    for(size_t i=a+1;i<r_tokens[a].next;i=r_tokens[i].next)items[out.count++]=(RusticKeyEvent){r_string(r_field(i,"key")),r_string(r_field(i,"state")),r_bool(r_field(i,"repeat"))};
    return out;
}
static const struct {
    struct {void (*EditAttribute)(const char*,const char*,const char*,RusticValue);} game;
    const char *(*entity_id)(void); double (*delta_time)(void),(*fixed_delta_time)(void);
    RusticVector3 (*get_translation)(void); void (*set_translation)(double,double,double);
    RusticValue (*get_property)(const char*),(*get_attribute)(const char*);
    void (*set_property)(const char*,RusticValue),(*edit_attribute)(const char*,RusticValue);
    RusticActionState (*input)(const char*),(*key)(const char*);RusticKeyEvents (*key_events)(void);
    bool (*any_key_pressed)(void);void (*log)(const char*,const char*),(*set_enabled)(bool);
} rustic={{r_edit_object_attribute},r_entity_id,r_delta_time,r_fixed_delta_time,r_translation,r_set_translation,r_property,r_attribute,r_set_property,r_edit_attribute,r_input,r_key,r_events,r_any,r_log,r_enabled};
static const struct {void (*add)(const char*,const char*),(*clone)(const char*,const char*);} instance={r_add,r_clone};
static const struct {struct {const char *(*Find)(const char*);RusticList (*List)(const char*);} scene;void (*setCurrentCamera)(const char*);} Game={{r_find,r_list},r_camera};
static int rustic_run(RusticBehavior b) {
    /* Reference exported objects even in an empty behavior to support -Werror. */
    (void)rustic;(void)instance;(void)Game;
    while(fgets(r_source,sizeof r_source,stdin)) {
        if(!strchr(r_source,'\n'))r_fail("Rustic C: oversized or unterminated state");
        for(size_t i=0;i<r_count;++i){free(r_tokens[i].decoded);r_tokens[i].decoded=NULL;}
        r_count=r_at=r_written=r_commands=0;(void)r_parse(0);r_append("{\"format_version\":1,\"commands\":[");
        const char *cb=r_string(r_field(0,"callback"));double dt=r_number(r_field(0,"delta"));
        if(!strcmp(cb,"on_create")&&b.on_create)b.on_create();else if(!strcmp(cb,"on_start")&&b.on_start)b.on_start();
        else if(!strcmp(cb,"on_enable")&&b.on_enable)b.on_enable();else if(!strcmp(cb,"on_disable")&&b.on_disable)b.on_disable();
        else if(!strcmp(cb,"on_destroy")&&b.on_destroy)b.on_destroy();else if(!strcmp(cb,"on_stop")&&b.on_stop)b.on_stop();
        else if(!strcmp(cb,"fixed_update")&&b.fixed_update)b.fixed_update(dt);else if(!strcmp(cb,"update")&&b.update)b.update(dt);
        r_append("]}");puts(r_output);fflush(stdout);
    }
    for(size_t i=0;i<r_count;++i){free(r_tokens[i].decoded);r_tokens[i].decoded=NULL;}return 0;
}
