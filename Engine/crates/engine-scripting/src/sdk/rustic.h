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
typedef struct { RusticType type; bool boolean; int64_t integer; double number; const char *string; double vector[4]; size_t length; } RusticValue;
typedef struct { bool pressed,released,held; double axis; } RusticActionState;
typedef struct { const char *key,*state; bool repeat; } RusticKeyEvent;
typedef struct { size_t count; const char **items; } RusticList;
typedef struct { const char *id,*name; } RusticAudioDevice;
typedef struct { size_t count; RusticAudioDevice *items; } RusticAudioDevices;
typedef struct { size_t count; RusticKeyEvent *items; } RusticKeyEvents;
typedef struct {char id[64];bool recording;} RusticCaptureHandle;
typedef struct {size_t count;double *samples;unsigned sampleRate,channels;bool active;size_t droppedSamples;} RusticCaptureData;
static RusticCaptureHandle r_recording_start(const char*,const char*);
static void r_recording_stop(const RusticCaptureHandle*);
static RusticCaptureHandle r_mic_start(void);
static void r_mic_stop(const RusticCaptureHandle*);
static RusticValue r_object_attribute(const char*,const char*);
static const char *r_find_live(const char*);
static void r_edit_object_path(const char*,const char*,RusticValue);
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
    else if(k=='[') {v.type=RUSTIC_VECTOR;for(size_t j=i+1;j<r_tokens[i].next&&v.length<4;j=r_tokens[j].next)v.vector[v.length++]=r_number(j);}
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
static void r_write_value(RusticValue v) {switch(v.type){case RUSTIC_BOOL:r_append(v.boolean?"true":"false");break;case RUSTIC_INTEGER:{char b[64];snprintf(b,sizeof b,"%" PRId64,v.integer);r_append(b);break;}case RUSTIC_NUMBER:r_double(v.number);break;case RUSTIC_STRING:r_quote(v.string);break;case RUSTIC_VECTOR:r_append("[");if(v.length>4)r_fail("Rustic C: invalid vector");for(size_t i=0;i<v.length;++i){if(i)r_append(",");r_double(v.vector[i]);}r_append("]");break;default:r_append("null");}}
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
static void r_print(const char *message){r_log("info",message);}
static void r_warn(const char *message){r_log("warn",message);}
static void r_enabled(bool e) {r_command("set_enabled");r_append(e?",\"enabled\":true}":",\"enabled\":false}");}
static void r_instance(const char *op,const char *s,const char *p) {r_command(op);r_append(",\"source\":");r_quote(s);r_append(",\"parent\":");r_quote(p);r_append("}");}
static void r_add(const char *s,const char *p) {r_instance("add_instance",s,p);}
static void r_clone(const char *s,const char *p) {r_instance("clone_instance",s,p);}
static void r_camera(const char *s) {r_command("set_current_camera");r_append(",\"source\":");r_quote(s);r_append("}");}
static const char *r_find(const char *s) {return r_string(r_field(r_field(0,"scene_paths"),s));}
static RusticList r_list(const char *path) {
    static const char *items[65536]; RusticList out={0,items}; size_t o=r_field(0,"scene_paths"); if(o>=r_count)return out;
    if(!path||!*path){path="Game.scene";}
    size_t n=strlen(path);
    for(size_t i=o+1;i<r_tokens[o].next;i=r_tokens[i+1].next) {const char *name=r_string(i);if(strncmp(name,path,n)==0&&name[n]=='.')items[out.count++]=r_string(i+1);}
    return out;
}
static RusticKeyEvents r_events(void) {
    static RusticKeyEvent items[65536];RusticKeyEvents out={0,items};size_t a=r_field(0,"key_events");if(a>=r_count)return out;
    for(size_t i=a+1;i<r_tokens[a].next;i=r_tokens[i].next)items[out.count++]=(RusticKeyEvent){r_string(r_field(i,"key")),r_string(r_field(i,"state")),r_bool(r_field(i,"repeat"))};
    return out;
}
static const struct {
    struct {void (*EditAttribute)(const char*,const char*,const char*,RusticValue);
    RusticValue (*GetAttribute)(const char*,const char*);
    RusticCaptureHandle (*startRecording)(const char*,const char*);
    void (*stopRecording)(const RusticCaptureHandle*);
    RusticCaptureHandle (*startHotMic)(void);void (*stopHotMic)(const RusticCaptureHandle*);} game;
    const char *(*entity_id)(void); double (*delta_time)(void),(*fixed_delta_time)(void);
    RusticVector3 (*get_translation)(void); void (*set_translation)(double,double,double);
    RusticValue (*get_property)(const char*),(*get_attribute)(const char*);
    void (*set_property)(const char*,RusticValue),(*edit_attribute)(const char*,RusticValue);
    RusticActionState (*input)(const char*),(*key)(const char*);RusticKeyEvents (*key_events)(void);
    bool (*any_key_pressed)(void);void (*log)(const char*,const char*),(*set_enabled)(bool);
    const char *(*find_entity)(const char*);RusticValue (*get_object_attribute)(const char*,const char*);void (*edit_object_attribute)(const char*,const char*,RusticValue);void (*print)(const char*),(*warn)(const char*);
} rustic={{r_edit_object_attribute,r_object_attribute,r_recording_start,r_recording_stop,r_mic_start,r_mic_stop},r_entity_id,r_delta_time,r_fixed_delta_time,r_translation,r_set_translation,r_property,r_attribute,r_set_property,r_edit_attribute,r_input,r_key,r_events,r_any,r_log,r_enabled,r_find_live,r_object_attribute,r_edit_object_path,r_print,r_warn};
static const struct {void (*add)(const char*,const char*),(*clone)(const char*,const char*);} instance={r_add,r_clone};

static RusticValue r_environment_get(const char *field);
static void r_environment_set(const char *field,RusticValue value);
static void r_environment_sky(const char *path);
static const struct {struct {RusticValue (*getEnvironment)(const char*);void (*setEnvironment)(const char*,RusticValue);void (*setSkyTexture)(const char*);const char *(*Find)(const char*);RusticList (*List)(const char*);} scene;void (*setCurrentCamera)(const char*);
RusticCaptureHandle (*startRecording)(const char*,const char*);void (*stopRecording)(const RusticCaptureHandle*);
RusticCaptureHandle (*startHotMic)(void);void (*stopHotMic)(const RusticCaptureHandle*);} Game={{r_environment_get,r_environment_set,r_environment_sky,r_find,r_list},r_camera,r_recording_start,r_recording_stop,r_mic_start,r_mic_stop};

/* Queries preserve the lifecycle snapshot while flushing earlier writes in order. */
typedef struct {char *source,*pending;RToken *tokens;size_t count,at;bool audio_devices;} RQuerySave;
typedef struct {bool hit;char entity[64];RusticVector3 point,normal;double distance;} RusticHit;
typedef struct {double value,velocity;} RusticDamping;
typedef struct {bool found;char status[16],error[256];} RusticOperationState;
typedef struct {double scale;bool paused;} RusticClockState;
typedef struct {RusticClockState clock;RusticOperationState operation;RusticValue value;RusticHit hit;RusticDamping damping;char *json;RusticList list;RusticAudioDevices devices;RusticCaptureData capture;} RQueryResult;
static char *r_query_owned[8192];static size_t r_query_owned_count;
static char *r_owned(const char *s,size_t length){if(r_query_owned_count>=8192)r_fail("Rustic C: query allocation budget");char *copy=(char*)malloc(length+1);if(!copy)r_fail("Rustic C: allocation failed");if(s)memcpy(copy,s,length);else memset(copy,0,length);copy[length]=0;r_query_owned[r_query_owned_count++]=copy;return copy;}
static RQuerySave r_query_begin(const char *op){
    RQuerySave saved={0};saved.audio_devices=strcmp(op,"audio_get_output_devices")==0||strcmp(op,"audio_get_input_devices")==0;saved.source=(char*)malloc(strlen(r_source)+1);saved.pending=(char*)malloc(r_written+1);saved.tokens=(RToken*)malloc(r_count*sizeof(RToken));
    if(!saved.source||!saved.pending||!saved.tokens)r_fail("Rustic C: query snapshot allocation failed");
    strcpy(saved.source,r_source);memcpy(saved.pending,r_output,r_written+1);memcpy(saved.tokens,r_tokens,r_count*sizeof(RToken));saved.count=r_count;saved.at=r_at;
    r_written=0;r_output[0]=0;r_append("{\"op\":");r_quote(op);return saved;
}
static void r_q_number(const char *key,double v){r_append(",");r_quote(key);r_append(":");r_double(v);}
static void r_q_string(const char *key,const char *v){r_append(",");r_quote(key);r_append(":");r_quote(v);}
static void r_q_value(const char *key,RusticValue v){r_append(",");r_quote(key);r_append(":");r_write_value(v);}
static RusticValue r_vec(RusticVector3 v){return (RusticValue){.type=RUSTIC_VECTOR,.length=3,.vector={v.x,v.y,v.z}};}
static RusticVector3 r_as_vec(RusticValue v){return (RusticVector3){v.vector[0],v.vector[1],v.vector[2]};}
static const char *r_resolve(const char *e);
static RQueryResult r_query_end(RQuerySave saved){
    r_append("}");fputs("{\"query\":",stdout);fputs(r_output,stdout);fputs(",\"commands\":",stdout);
    const char *array=strchr(saved.pending,'[');if(array)fputs(array,stdout);else fputs("[",stdout);fputs("]}\n",stdout);fflush(stdout);
    if(!fgets(r_source,sizeof r_source,stdin)||!strchr(r_source,'\n'))r_fail("Rustic C: missing or oversized query response");
    memset(r_tokens,0,sizeof r_tokens);r_count=r_at=0;(void)r_parse(0);size_t error=r_field(0,"error");if(error<r_count)r_fail(r_string(error));
    size_t result=r_field(0,"result");RQueryResult out={0};
    if(result<r_count){out.json=r_owned(r_source+r_tokens[result].start,r_tokens[result].end-r_tokens[result].start);
        if(r_tokens[result].kind=='{'){
            out.clock.scale=r_number(r_field(result,"scale"));out.clock.paused=r_bool(r_field(result,"paused"));
            size_t samples=r_field(result,"samples");
            if(samples<r_count&&r_tokens[samples].kind=='['){
                for(size_t i=samples+1;i<r_tokens[samples].next;i=r_tokens[i].next)out.capture.count++;
                out.capture.samples=(double*)r_owned(NULL,out.capture.count*sizeof(double));
                size_t n=0;for(size_t i=samples+1;i<r_tokens[samples].next;i=r_tokens[i].next)out.capture.samples[n++]=r_number(i);
                out.capture.sampleRate=(unsigned)r_number(r_field(result,"sampleRate"));out.capture.channels=(unsigned)r_number(r_field(result,"channels"));
                out.capture.active=r_bool(r_field(result,"active"));out.capture.droppedSamples=(size_t)r_number(r_field(result,"droppedSamples"));
            }
            size_t status=r_field(result,"status");if(status<r_count){out.operation.found=true;snprintf(out.operation.status,sizeof out.operation.status,"%s",r_string(status));const char *message=r_string(r_field(result,"error"));if(message)snprintf(out.operation.error,sizeof out.operation.error,"%s",message);}
            size_t entity=r_field(result,"entity");if(entity<r_count){out.hit.hit=true;snprintf(out.hit.entity,sizeof out.hit.entity,"%s",r_string(entity));out.hit.point=r_as_vec(r_value(r_field(result,"point")));out.hit.normal=r_as_vec(r_value(r_field(result,"normal")));out.hit.distance=r_number(r_field(result,"distance"));}
            out.damping.value=r_number(r_field(result,"value"));out.damping.velocity=r_number(r_field(result,"velocity"));
        }else{out.value=r_value(result);if(out.value.type==RUSTIC_STRING)out.value.string=r_owned(out.value.string,strlen(out.value.string));
            if(saved.audio_devices&&r_tokens[result].kind=='['&&result+1<r_tokens[result].next){
                size_t count=0;for(size_t i=result+1;i<r_tokens[result].next;i=r_tokens[i].next)count++;
                RusticAudioDevice *items=(RusticAudioDevice*)r_owned(NULL,count*sizeof(RusticAudioDevice));
                size_t n=0;for(size_t i=result+1;i<r_tokens[result].next;i=r_tokens[i].next){
                    const char *id=r_string(r_field(i,"id")),*name=r_string(r_field(i,"name"));
                    if(!id||!name)r_fail("Rustic C: invalid audio device response");
                    items[n++]=(RusticAudioDevice){r_owned(id,strlen(id)),r_owned(name,strlen(name))};
                }out.devices=(RusticAudioDevices){count,items};
            }
            if(r_tokens[result].kind=='['&&result+1<r_tokens[result].next&&r_tokens[result+1].kind=='"'){
                size_t count=0;for(size_t i=result+1;i<r_tokens[result].next;i=r_tokens[i].next)count++;
                const char **items=(const char**)r_owned(NULL,count*sizeof(char*));
                size_t n=0;for(size_t i=result+1;i<r_tokens[result].next;i=r_tokens[i].next){const char *v=r_string(i);items[n++]=r_owned(v,strlen(v));}out.list=(RusticList){count,items};
            }
        }
    }
    for(size_t i=0;i<r_count;i++)free(r_tokens[i].decoded);
    strcpy(r_source,saved.source);memcpy(r_tokens,saved.tokens,saved.count*sizeof(RToken));r_count=saved.count;r_at=saved.at;
    free(saved.source);free(saved.pending);free(saved.tokens);r_written=r_commands=0;r_output[0]=0;r_append("{\"format_version\":1,\"commands\":[");return out;
}
static void r_edit_object_path(const char *source,const char *name,RusticValue value){r_command("edit_attribute");r_append(",\"source\":");r_quote(source);r_append(",\"name\":");r_quote(name);r_append(",\"value\":");r_write_value(value);r_append("}");}
static RusticValue r_object_attribute(const char *source,const char *name){RQuerySave q=r_query_begin("object_attribute");r_q_string("source",source);r_q_string("name",name);return r_query_end(q).value;}
static const char *r_find_live(const char *source){RQuerySave q=r_query_begin("find_entity");r_q_string("source",source);return r_query_end(q).value.string;}
static RusticCaptureHandle r_capture_start(bool recording,const char *path,const char *source){
 RQuerySave q=r_query_begin(recording?"capture_recording_start":"capture_mic_start");
 if(recording){r_q_string("path",path);r_q_string("source",source?source:"game");}
 RQueryResult result=r_query_end(q);RusticCaptureHandle h={{0},recording};
 if(result.value.type!=RUSTIC_STRING)r_fail("Rustic C: invalid capture handle");
 snprintf(h.id,sizeof h.id,"%s",result.value.string);return h;
}
static RusticCaptureHandle r_recording_start(const char *path,const char *source){return r_capture_start(true,path,source);}
static RusticCaptureHandle r_mic_start(void){return r_capture_start(false,NULL,NULL);}
static void r_capture_stop_kind(const RusticCaptureHandle *h,bool recording){RQuerySave q=r_query_begin(recording?"capture_recording_stop":"capture_mic_stop");r_q_string("id",h?h->id:NULL);(void)r_query_end(q);}
static void r_recording_stop(const RusticCaptureHandle *h){r_capture_stop_kind(h,true);}
static void r_mic_stop(const RusticCaptureHandle *h){r_capture_stop_kind(h,false);}
static void r_capture_stop(RusticCaptureHandle h){r_capture_stop_kind(&h,h.recording);}
static void r_capture_unload(RusticCaptureHandle h){RQuerySave q=r_query_begin("capture_unload");r_q_string("id",h.id);(void)r_query_end(q);}
static RusticCaptureData r_capture_read(RusticCaptureHandle h){RQuerySave q=r_query_begin("capture_read");r_q_string("id",h.id);return r_query_end(q).capture;}
static const struct {RusticCaptureData(*read)(RusticCaptureHandle);void(*stop)(RusticCaptureHandle),(*unload)(RusticCaptureHandle);} Capture={r_capture_read,r_capture_stop,r_capture_unload};
/* Entity services: returned lists/strings live until the next callback. */
static const char *r_resolve(const char *e){
    if(e && !strncmp(e,"game.scene.",11)){RQuerySave saved=r_query_begin("scene_object");r_q_string("source",e);return r_query_end(saved).value.string;}
    const char *id=e?r_find(e):NULL;return id?id:e?e:r_entity_id();
}
static bool r_tags_add(const char* entity,const char* name){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("tags_add");r_q_string("entity",resolved_entity);r_q_string("name",name);return r_query_end(s).value.boolean;}
static bool r_tags_remove(const char* entity,const char* name){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("tags_remove");r_q_string("entity",resolved_entity);r_q_string("name",name);return r_query_end(s).value.boolean;}
static RusticList r_tags_find_all(const char* name){RQuerySave s=r_query_begin("tags_find_all");r_q_string("name",name);return r_query_end(s).list;}
static const struct {bool(*add)(const char*,const char*);bool(*remove)(const char*,const char*);RusticList(*find_all)(const char*);} Tags={r_tags_add,r_tags_remove,r_tags_find_all};
static bool r_groups_create(const char* name){RQuerySave s=r_query_begin("groups_create");r_q_string("name",name);return r_query_end(s).value.boolean;}
static bool r_groups_add(const char* name,const char* entity){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("groups_add");r_q_string("name",name);r_q_string("entity",resolved_entity);return r_query_end(s).value.boolean;}
static RusticList r_groups_get_members(const char* name){RQuerySave s=r_query_begin("groups_get_members");r_q_string("name",name);return r_query_end(s).list;}
static const struct {bool(*create)(const char*);bool(*add)(const char*,const char*);RusticList(*get_members)(const char*);} Groups={r_groups_create,r_groups_add,r_groups_get_members};
static void r_hierarchy_set_parent(const char* entity,const char* parent,bool keep_world){const char *resolved_entity=r_resolve(entity);const char *resolved_parent=parent?r_resolve(parent):NULL;RQuerySave s=r_query_begin("hierarchy_set_parent");r_q_string("entity",resolved_entity);if(resolved_parent)r_q_string("parent",resolved_parent);else r_append(",\"parent\":null");r_append(keep_world?",\"keep_world\":true":",\"keep_world\":false");(void)r_query_end(s);}
static RusticList r_hierarchy_get_children(const char* entity){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("hierarchy_get_children");r_q_string("entity",resolved_entity);return r_query_end(s).list;}
static void r_hierarchy_detach(const char* entity,bool keep_world){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("hierarchy_detach");r_q_string("entity",resolved_entity);r_append(keep_world?",\"keep_world\":true":",\"keep_world\":false");(void)r_query_end(s);}
static const struct {void(*set_parent)(const char*,const char*,bool);RusticList(*get_children)(const char*);void(*detach)(const char*,bool);} Hierarchy={r_hierarchy_set_parent,r_hierarchy_get_children,r_hierarchy_detach};
static void r_entity_destroy(const char* entity){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("entity_destroy");r_q_string("entity",resolved_entity);(void)r_query_end(s);}
static void r_entity_destroy_after(const char* entity,double delay){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("entity_destroy_after");r_q_string("entity",resolved_entity);r_q_number("delay",delay);(void)r_query_end(s);}
static bool r_entity_is_valid(const char* entity){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("entity_is_valid");r_q_string("entity",resolved_entity);return r_query_end(s).value.boolean;}
static const struct {void(*destroy)(const char*);void(*destroy_after)(const char*,double);bool(*is_valid)(const char*);} Entity={r_entity_destroy,r_entity_destroy_after,r_entity_is_valid};
static bool r_components_has(const char* entity,const char* name){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("components_has");r_q_string("entity",resolved_entity);r_q_string("name",name);return r_query_end(s).value.boolean;}
static RusticList r_components_get_all(const char* entity){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("components_get_all");r_q_string("entity",resolved_entity);return r_query_end(s).list;}
static RusticList r_components_find_in_children(const char* entity,const char* name){const char *resolved_entity=r_resolve(entity);RQuerySave s=r_query_begin("components_find_in_children");r_q_string("entity",resolved_entity);r_q_string("name",name);return r_query_end(s).list;}
static const struct {bool(*has)(const char*,const char*);RusticList(*get_all)(const char*);RusticList(*find_in_children)(const char*,const char*);} Components={r_components_has,r_components_get_all,r_components_find_in_children};
static RusticList r_world_find_by_tag(const char* name){RQuerySave s=r_query_begin("world_find_by_tag");r_q_string("name",name);return r_query_end(s).list;}
static RusticList r_world_find_in_radius(RusticVector3 center,double radius){RQuerySave s=r_query_begin("world_find_in_radius");r_q_value("center",r_vec(center));r_q_number("radius",radius);return r_query_end(s).list;}
static const struct {RusticList(*find_by_tag)(const char*);RusticList(*find_in_radius)(RusticVector3,double);} World={r_world_find_by_tag,r_world_find_in_radius};

static RusticValue r_environment_get(const char *field){RQuerySave s=r_query_begin("scene_environment_field");r_q_string("field",field);return r_query_end(s).value;}
static void r_environment_set(const char *field,RusticValue value){RQuerySave s=r_query_begin("scene_environment");r_append(",\"settings\":{");r_quote(field);r_append(":");r_write_value(value);r_append("}");(void)r_query_end(s);}
static void r_environment_sky(const char *path){r_environment_set("sky_image",(RusticValue){.type=RUSTIC_STRING,.string=path});}
static RusticValue r_smooth_lerp(RusticValue a,RusticValue b,double progress,const char *easing){RQuerySave s=r_query_begin("lerp");r_q_value("from",a);r_q_value("to",b);r_q_number("progress",progress);r_q_string("easing",easing?easing:"Linear");return r_query_end(s).value;}
static RusticValue r_smooth_slerp(RusticValue a,RusticValue b,double progress,const char *easing){RQuerySave s=r_query_begin("slerp");r_q_value("from",a);r_q_value("to",b);r_q_number("progress",progress);r_q_string("easing",easing?easing:"Linear");return r_query_end(s).value;}
static double r_inverse_lerp(double a,double b,double v){RQuerySave s=r_query_begin("inverse_lerp");r_q_number("from",a);r_q_number("to",b);r_q_number("value",v);RusticValue result=r_query_end(s).value;return result.type==RUSTIC_INTEGER?(double)result.integer:result.number;}
static double r_remap(double v,double a,double b,double c,double d){RQuerySave s=r_query_begin("remap");r_q_number("value",v);r_q_number("in_min",a);r_q_number("in_max",b);r_q_number("out_min",c);r_q_number("out_max",d);RusticValue result=r_query_end(s).value;return result.type==RUSTIC_INTEGER?(double)result.integer:result.number;}
static RusticDamping r_smooth_damp(double current,double target,double velocity,double smoothTime,double delta){RQuerySave s=r_query_begin("smooth_damp");r_q_number("current",current);r_q_number("target",target);r_q_number("velocity",velocity);r_q_number("smooth_time",smoothTime);r_q_number("delta",delta);return r_query_end(s).damping;}
static RusticHit r_sphere_cast(RusticVector3 origin,RusticVector3 direction,double distance,double radius){RQuerySave s=r_query_begin("physics_sphere_cast");r_q_value("origin",r_vec(origin));r_q_value("direction",r_vec(direction));r_q_number("distance",distance);r_q_number("radius",radius);return r_query_end(s).hit;}
static RusticHit r_raycast(RusticVector3 origin,RusticVector3 direction,double distance){return r_sphere_cast(origin,direction,distance,0);}
static RusticList r_overlap(RusticVector3 center,double radius){RQuerySave s=r_query_begin("physics_overlap");r_q_value("center",r_vec(center));r_q_number("radius",radius);return r_query_end(s).list;}
static void r_physics_vector(const char *op,const char *entity,RusticVector3 vector,double delta){const char *id=r_resolve(entity);RQuerySave s=r_query_begin(op);r_q_string("entity",id);r_q_value("vector",r_vec(vector));r_q_number("delta",delta);(void)r_query_end(s);}
static void r_impulse(const char *entity,RusticVector3 v){r_physics_vector("physics_impulse",entity,v,0);}
static void r_launch(const char *entity,RusticVector3 v){r_physics_vector("physics_launch",entity,v,0);}
static void r_force(const char *entity,RusticVector3 v,double delta){r_physics_vector("physics_force",entity,v,delta);}
static RusticList r_explosion(RusticVector3 center,double radius,double strength){RQuerySave s=r_query_begin("physics_explosion");r_q_value("center",r_vec(center));r_q_number("radius",radius);r_q_number("strength",strength);return r_query_end(s).list;}
static const struct {RusticValue(*lerp)(RusticValue,RusticValue,double,const char*),(*slerp)(RusticValue,RusticValue,double,const char*);double(*inverseLerp)(double,double,double),(*remap)(double,double,double,double,double);RusticDamping(*smoothDamp)(double,double,double,double,double);} Smooth={r_smooth_lerp,r_smooth_slerp,r_inverse_lerp,r_remap,r_smooth_damp},Interpolation={r_smooth_lerp,r_smooth_slerp,r_inverse_lerp,r_remap,r_smooth_damp};
static const struct {RusticHit(*raycast)(RusticVector3,RusticVector3,double),(*sphereCast)(RusticVector3,RusticVector3,double,double);RusticList(*overlap)(RusticVector3,double);void(*impulse)(const char*,RusticVector3),(*launch)(const char*,RusticVector3),(*knockback)(const char*,RusticVector3),(*force)(const char*,RusticVector3,double);RusticList(*explosion)(RusticVector3,double,double);} Physics={r_raycast,r_sphere_cast,r_overlap,r_impulse,r_launch,r_impulse,r_force,r_explosion};
/* High-level tween/timer handles own no world state. */
typedef uint64_t RusticGameplayHandle;
typedef struct { RusticGameplayHandle id,owner; void (*finished)(void);void(*arguments)(const RusticValue*,size_t);void(*value)(RusticValue);bool persistent,signal; } RGameplaySlot;
static RGameplaySlot r_gameplay_slots[4096];
typedef struct {RusticGameplayHandle owner;char name[257];void(*fn)(void);} RMarkerSlot;
static RMarkerSlot r_marker_slots[4096];static RusticGameplayHandle r_dispatch_owner;
static void r_marker_dispatch(const RusticValue *args,size_t count){if(!count||args[0].type!=RUSTIC_STRING)return;for(size_t i=0;i<4096;i++)if(r_marker_slots[i].owner==r_dispatch_owner&&!strcmp(r_marker_slots[i].name,args[0].string)){void(*fn)(void)=r_marker_slots[i].fn;if(fn)fn();}}

static RusticGameplayHandle r_gameplay_serial;
static void r_gameplay_handle(RusticGameplayHandle id) {char b[64];snprintf(b,sizeof b,"gameplay-%" PRIu64,id);r_quote(b);}
static RusticGameplayHandle r_gameplay_new(void (*finished)(void)) {
    if(r_gameplay_serial==UINT64_MAX)r_fail("Rustic C: gameplay handle overflow");
    RusticGameplayHandle id=++r_gameplay_serial;
    for(size_t i=0;i<4096;++i)if(!r_gameplay_slots[i].id){r_gameplay_slots[i]=(RGameplaySlot){.id=id,.finished=finished};return id;}
    r_fail("Rustic C: gameplay handle limit");return 0;
}
static void r_gameplay_start(RusticGameplayHandle id) {
    r_command("gameplay");r_append(",\"request\":{\"command\":\"start\",\"handle\":");r_gameplay_handle(id);r_append(",\"on_finished\":");r_gameplay_handle(id);r_append(",\"action\":");
}
static RusticGameplayHandle r_tween_to(const char *entity,const char *property,RusticValue to,double duration,const char *easing,void (*finished)(void)) {
    RusticGameplayHandle id=r_gameplay_new(finished);const char *resolved=r_resolve(entity);
    r_gameplay_start(id);r_append("{\"kind\":\"tween\",\"target\":{\"entity\":");r_quote(resolved?resolved:entity?entity:r_entity_id());
    r_append(",\"property\":");r_quote(property);r_append("},\"to\":");r_write_value(to);
    r_append(",\"duration\":");r_double(duration);r_append(",\"easing\":");r_quote(easing?easing:"Linear");r_append("}}}");return id;
}
static RusticGameplayHandle r_tween_move(const char *entity,RusticVector3 p,double duration,const char *easing,void (*finished)(void)) {
    RusticValue value={.type=RUSTIC_VECTOR,.vector={p.x,p.y,p.z},.length=3};return r_tween_to(entity,"Position",value,duration,easing,finished);
}
static RusticGameplayHandle r_timer_after(double duration,void (*finished)(void)) {
    RusticGameplayHandle id=r_gameplay_new(finished);r_gameplay_start(id);r_append("{\"kind\":\"wait\",\"duration\":");r_double(duration);r_append("}}}");return id;
}
static void r_gameplay_control(RusticGameplayHandle id,const char *command) {
    r_command("gameplay");r_append(",\"request\":{\"command\":");r_quote(command);r_append(",\"handle\":");r_gameplay_handle(id);r_append("}}");
    if(!strcmp(command,"cancel"))for(size_t i=0;i<4096;++i){if(r_gameplay_slots[i].id==id||r_gameplay_slots[i].owner==id)r_gameplay_slots[i].id=0;if(r_marker_slots[i].owner==id)r_marker_slots[i].owner=0;}
}
static void r_gameplay_pause(RusticGameplayHandle id){r_gameplay_control(id,"pause");}
static void r_gameplay_resume(RusticGameplayHandle id){r_gameplay_control(id,"resume");}
static void r_gameplay_cancel(RusticGameplayHandle id){r_gameplay_control(id,"cancel");}
static void r_gameplay_reverse(RusticGameplayHandle id){r_gameplay_control(id,"reverse");}
static void r_gameplay_dispatch(void) {
    size_t connections=r_field(0,"gameplay_connections");
    for(size_t i=0;i<4096;i++)if(r_gameplay_slots[i].id&&r_gameplay_slots[i].signal){bool live=false;if(connections<r_count)for(size_t n=connections+1;n<r_tokens[connections].next;n=r_tokens[n].next){const char *token=r_string(n);if(token&&!strncmp(token,"gameplay-",9)&&(RusticGameplayHandle)strtoull(token+9,NULL,10)==r_gameplay_slots[i].id){live=true;break;}}if(!live)r_gameplay_slots[i].id=0;}

    RusticGameplayHandle previous[4096];for(size_t i=0;i<4096;++i)previous[i]=r_gameplay_slots[i].id;
    size_t a=r_field(0,"gameplay_callbacks");
    if(a<r_count)for(size_t item=a+1;item<r_tokens[a].next;item=r_tokens[item].next){
        const char *token=r_string(r_field(item,"token"));if(!token||strncmp(token,"gameplay-",9))continue;
        RusticGameplayHandle id=(RusticGameplayHandle)strtoull(token+9,NULL,10);
        for(size_t i=0;i<4096;++i)if(r_gameplay_slots[i].id==id){RGameplaySlot slot=r_gameplay_slots[i];if(!slot.persistent)r_gameplay_slots[i].id=0;
            size_t args=r_field(item,"arguments");RusticValue values[64];size_t count=0;if(args<r_count)for(size_t n=args+1;n<r_tokens[args].next&&count<64;n=r_tokens[n].next)values[count++]=r_value(n);
            r_dispatch_owner=slot.owner;
            if(slot.arguments)slot.arguments(values,count);else if(slot.value&&count)slot.value(values[0]);else if(slot.finished)slot.finished();
            if(!slot.owner&&!slot.signal){for(size_t n=0;n<4096;n++){if(r_gameplay_slots[n].owner==id)r_gameplay_slots[n].id=0;if(r_marker_slots[n].owner==id)r_marker_slots[n].owner=0;}}
            break;
        }
    }
    size_t states=r_field(0,"gameplay_states");
    if(states<r_count)for(size_t i=0;i<4096;++i)if(previous[i]&&r_gameplay_slots[i].id==previous[i]&&!r_gameplay_slots[i].signal){
        bool alive=false;
        for(size_t item=states+1;item<r_tokens[states].next;item=r_tokens[item].next){
            const char *handle=r_string(r_field(item,"handle"));if(!handle||strncmp(handle,"gameplay-",9))continue;
            if((RusticGameplayHandle)strtoull(handle+9,NULL,10)!=(r_gameplay_slots[i].owner?r_gameplay_slots[i].owner:r_gameplay_slots[i].id))continue;
            const char *status=r_string(r_field(item,"status"));alive=status&&(!strcmp(status,"running")||!strcmp(status,"paused"));break;
        }
        if(!alive){RusticGameplayHandle owner=r_gameplay_slots[i].owner?r_gameplay_slots[i].owner:r_gameplay_slots[i].id;r_gameplay_slots[i].id=0;for(size_t n=0;n<4096;n++)if(r_marker_slots[n].owner==owner)r_marker_slots[n].owner=0;}
    }
}
/* Serializable action values remain private to this SDK. */
typedef struct {char *private_data;RusticGameplayHandle private_tokens[128];size_t private_count;} RusticAction;
typedef struct {RusticAction *private_actions;size_t count,capacity;bool played;} RusticSequence;
typedef struct {char *private_data;} RusticPath;
typedef struct {RusticVector3 point;const char *easing;} RusticPathPoint;
typedef struct {char *output;size_t written,commands;} RActionSave;
static RActionSave r_action_begin(void){RActionSave s={(char*)malloc(r_written+1),r_written,r_commands};if(!s.output)r_fail("Rustic C: action allocation failed");memcpy(s.output,r_output,r_written+1);r_written=0;r_output[0]=0;return s;}
static RusticAction r_action_end(RActionSave s){RusticAction a={0};a.private_data=(char*)malloc(r_written+1);if(!a.private_data)r_fail("Rustic C: action allocation failed");memcpy(a.private_data,r_output,r_written+1);memcpy(r_output,s.output,s.written+1);r_written=s.written;r_commands=s.commands;free(s.output);return a;}
static void r_action_free(RusticAction *a){free(a->private_data);a->private_data=NULL;a->private_count=0;}
static void r_action_tokens(RusticAction *to,const RusticAction *from){if(to->private_count+from->private_count>128)r_fail("Rustic C: action callback limit");memcpy(to->private_tokens+to->private_count,from->private_tokens,from->private_count*sizeof(RusticGameplayHandle));to->private_count+=from->private_count;}
static RusticAction r_action_tween(const char *e,const char *property,RusticValue to,double duration,const char *easing){const char *entity=r_resolve(e);RActionSave s=r_action_begin();r_append("{\"kind\":\"tween\",\"target\":{\"entity\":");r_quote(entity);r_append(",\"property\":");r_quote(property);r_append("},\"to\":");r_write_value(to);r_q_number("duration",duration);r_q_string("easing",easing?easing:"Linear");r_append("}");return r_action_end(s);}
static RusticGameplayHandle r_start_action(RusticAction a,bool looping,unsigned repeats,bool pingPong,void(*finished)(void)){
    if(!a.private_data){r_fail("Rustic C: empty action");}
    RusticGameplayHandle id=r_gameplay_new(finished);
    for(size_t t=0;t<a.private_count;t++)for(size_t i=0;i<4096;i++)if(r_gameplay_slots[i].id==a.private_tokens[t])r_gameplay_slots[i].owner=id;
    r_gameplay_start(id);r_append(a.private_data);r_append(",\"playback\":{\"looping\":");r_append(looping?"true":"false");r_append(",\"repeats\":");char number[32];snprintf(number,sizeof number,"%u",repeats);r_append(number);r_append(",\"ping_pong\":");r_append(pingPong?"true":"false");r_append("}}}");return id;
}
static RusticGameplayHandle r_run_action(RusticAction a,void(*finished)(void)){RusticGameplayHandle h=r_start_action(a,false,0,false,finished);r_action_free(&a);return h;}
static RusticGameplayHandle r_rotate_to(const char *e,RusticValue q,double d,const char *ease,void(*fn)(void)){return r_tween_to(e,"Rotation",q,d,ease,fn);}
static RusticGameplayHandle r_scale_to(const char *e,RusticVector3 scale,double d,const char *ease,void(*fn)(void)){return r_tween_to(e,"Scale",r_vec(scale),d,ease,fn);}
static RusticAction r_motion_action(const char *kind,const char *e,const char *key,RusticVector3 vector,double duration,const char *ease){const char *id=r_resolve(e);RActionSave s=r_action_begin();r_append("{\"kind\":");r_quote(kind);r_q_string("entity",id);r_q_value(key,r_vec(vector));r_q_number("duration",duration);r_q_string("easing",ease?ease:"Linear");r_append("}");return r_action_end(s);}
static RusticGameplayHandle r_move_relative(const char *e,RusticVector3 offset,double d,const char *ease,void(*fn)(void)){return r_run_action(r_motion_action("move",e,"offset",offset,d,ease),fn);}
static RusticGameplayHandle r_look_at(const char *e,RusticVector3 target,double d,const char *ease,void(*fn)(void)){return r_run_action(r_motion_action("look_at",e,"position",target,d,ease),fn);}
static RusticGameplayHandle r_follow(const char *e,const char *target,RusticVector3 offset,double duration,const char *ease,void(*fn)(void)){const char *id=r_resolve(e),*other=r_resolve(target);RActionSave s=r_action_begin();r_append("{\"kind\":\"follow\"");r_q_string("entity",id);r_q_string("target",other);r_q_value("offset",r_vec(offset));r_q_number("duration",duration);r_q_string("easing",ease?ease:"Linear");r_append("}");return r_run_action(r_action_end(s),fn);}
static RusticGameplayHandle r_orbit(const char *e,RusticVector3 center,double radius,double turns,double duration,const char *ease,void(*fn)(void)){const char *id=r_resolve(e);RActionSave s=r_action_begin();r_append("{\"kind\":\"orbit\"");r_q_string("entity",id);r_q_value("center",r_vec(center));r_q_number("radius",radius);r_q_number("turns",turns);r_q_number("duration",duration);r_q_string("easing",ease?ease:"Linear");r_append("}");return r_run_action(r_action_end(s),fn);}
static RusticSequence r_sequence_new(void){return (RusticSequence){0};}
static RusticSequence *r_sequence_add(RusticSequence *b,RusticAction a){if(b->played||b->count>=4096)r_fail("Rustic C: sequence is single-use or full");if(b->count==b->capacity){b->capacity=b->capacity?b->capacity*2:8;b->private_actions=(RusticAction*)realloc(b->private_actions,b->capacity*sizeof(RusticAction));if(!b->private_actions)r_fail("Rustic C: sequence allocation failed");}b->private_actions[b->count++]=a;return b;}
static RusticSequence *r_sequence_to(RusticSequence *b,const char *e,const char *p,RusticValue v,double d,const char *ease){return r_sequence_add(b,r_action_tween(e,p,v,d,ease));}
static RusticSequence *r_sequence_move(RusticSequence *b,const char *e,RusticVector3 p,double d,const char *ease){return r_sequence_to(b,e,"Position",r_vec(p),d,ease);}
static RusticSequence *r_sequence_wait(RusticSequence *b,double duration){RActionSave s=r_action_begin();r_append("{\"kind\":\"wait\"");r_q_number("duration",duration);r_append("}");return r_sequence_add(b,r_action_end(s));}
static RusticSequence *r_sequence_call(RusticSequence *b,void(*fn)(void)){RusticGameplayHandle token=r_gameplay_new(fn);RActionSave s=r_action_begin();r_append("{\"kind\":\"callback\",\"token\":");r_gameplay_handle(token);r_append("}");RusticAction a=r_action_end(s);a.private_tokens[a.private_count++]=token;return r_sequence_add(b,a);}
static RusticAction r_sequence_action(RusticSequence *b,const char *kind){RActionSave s=r_action_begin();r_append("{\"kind\":");r_quote(kind);r_append(",\"actions\":[");RusticAction tokens={0};for(size_t i=0;i<b->count;i++){if(i)r_append(",");r_append(b->private_actions[i].private_data);r_action_tokens(&tokens,&b->private_actions[i]);}r_append("]}");RusticAction a=r_action_end(s);r_action_tokens(&a,&tokens);return a;}
static RusticSequence *r_sequence_parallel(RusticSequence *b,RusticSequence *branches,size_t count){RusticSequence temporary=r_sequence_new();for(size_t i=0;i<count;i++)r_sequence_add(&temporary,r_sequence_action(&branches[i],"sequence"));RusticAction a=r_sequence_action(&temporary,"parallel");for(size_t i=0;i<temporary.count;i++)r_action_free(&temporary.private_actions[i]);free(temporary.private_actions);return r_sequence_add(b,a);}
static void r_sequence_destroy(RusticSequence *b){for(size_t i=0;i<b->count;i++)r_action_free(&b->private_actions[i]);free(b->private_actions);*b=(RusticSequence){0};}
static RusticGameplayHandle r_sequence_play(RusticSequence *b,void(*fn)(void)){if(b->played)r_fail("Rustic C: sequence already played");b->played=true;return r_run_action(r_sequence_action(b,"sequence"),fn);}
static RusticGameplayHandle r_timer_every(double interval,void(*fn)(void),unsigned count){if(interval<=0)r_fail("Rustic C: positive timer interval required");RusticSequence b=r_sequence_new();r_sequence_wait(&b,interval);r_sequence_call(&b,fn);RusticGameplayHandle token=b.private_actions[1].private_tokens[0];for(size_t i=0;i<4096;i++)if(r_gameplay_slots[i].id==token)r_gameplay_slots[i].persistent=true;RusticAction a=r_sequence_action(&b,"sequence");RusticGameplayHandle id=r_start_action(a,count==0,count?count-1:0,false,NULL);r_action_free(&a);r_sequence_destroy(&b);return id;}
static RusticPath r_path_create(const RusticPathPoint *points,size_t count,const char *kind){RActionSave s=r_action_begin();r_append("{\"kind\":");r_quote(kind?kind:"Linear");r_append(",\"points\":[");for(size_t i=0;i<count;i++){if(i)r_append(",");r_append("{\"point\":");r_write_value(r_vec(points[i].point));r_q_string("easing",points[i].easing?points[i].easing:"Linear");r_append("}");}r_append("]}");RusticAction a=r_action_end(s);return (RusticPath){a.private_data};}
static void r_path_destroy(RusticPath *p){free(p->private_data);p->private_data=NULL;}
static RusticGameplayHandle r_path_follow(const char *e,RusticPath path,double duration,double speed,const char *ease,bool loop,bool pingPong,bool orient,void(*fn)(void)){const char *id=r_resolve(e);RActionSave s=r_action_begin();r_append("{\"kind\":\"path\"");r_q_string("entity",id);r_append(",\"path\":");r_append(path.private_data);r_q_number(speed>0?"speed":"duration",speed>0?speed:duration);r_q_string("easing",ease?ease:"Linear");r_append(",\"orient_to_path\":");r_append(orient?"true":"false");r_append("}");RusticAction a=r_action_end(s);RusticGameplayHandle h=r_start_action(a,loop,0,pingPong,fn);r_action_free(&a);return h;}
static RusticGameplayHandle r_fade(const char *e,double opacity,double duration,const char *ease,void(*fn)(void)){return r_tween_to(e,"Opacity",(RusticValue){.type=RUSTIC_NUMBER,.number=opacity},duration,ease,fn);}
static RusticGameplayHandle r_flash(const char *e,RusticVector3 color,double duration,const char *ease,void(*fn)(void)){RusticAction a=r_action_tween(e,"Color",r_vec(color),duration,ease);RusticGameplayHandle h=r_start_action(a,false,1,true,fn);r_action_free(&a);return h;}
static RusticGameplayHandle r_pulse(const char *e,RusticVector3 scale,double duration,const char *ease,void(*fn)(void)){RusticAction a=r_action_tween(e,"Scale",r_vec(scale),duration,ease);RusticGameplayHandle h=r_start_action(a,false,1,true,fn);r_action_free(&a);return h;}
static RusticGameplayHandle r_shake(const char *e,double strength,double duration,const char *ease,void(*fn)(void)){const char *id=r_resolve(e);RActionSave s=r_action_begin();r_append("{\"kind\":\"shake\"");r_q_string("entity",id);r_q_number("strength",strength);r_q_number("duration",duration);r_q_string("easing",ease?ease:"Linear");r_append("}");return r_run_action(r_action_end(s),fn);}
static RusticGameplayHandle r_camera_zoom(const char *e,double fov,double duration,const char *ease,void(*fn)(void)){return r_tween_to(e,"Fov",(RusticValue){.type=RUSTIC_NUMBER,.number=fov},duration,ease,fn);}
static RusticGameplayHandle r_camera_transition(const char *e,RusticVector3 p,RusticValue rotation,double fov,double duration,const char *ease,void(*fn)(void)){RusticSequence b=r_sequence_new();r_sequence_to(&b,e,"Position",r_vec(p),duration,ease);r_sequence_to(&b,e,"Rotation",rotation,duration,ease);r_sequence_to(&b,e,"Fov",(RusticValue){.type=RUSTIC_NUMBER,.number=fov},duration,ease);RusticGameplayHandle h=r_run_action(r_sequence_action(&b,"parallel"),fn);r_sequence_destroy(&b);return h;}
typedef struct {char id[64];} RusticAudioVoice;
typedef struct {double volume,pitch;bool loop;} RusticAudioOptions;
typedef struct {char id[64];} RusticLoadedAudio;
static RusticLoadedAudio r_audio_load(const char *source){RQuerySave q=r_query_begin("audio_load");r_q_string("source",source);RQueryResult result=r_query_end(q);RusticLoadedAudio h={{0}};if(result.value.type!=RUSTIC_STRING)r_fail("Rustic C: invalid loaded audio handle");snprintf(h.id,sizeof h.id,"%s",result.value.string);return h;}
static RusticAudioVoice r_audio_loaded_play(RusticLoadedAudio h,const RusticAudioOptions *opts){RQuerySave q=r_query_begin("audio_loaded_play");r_q_string("entity",h.id);r_q_number("volume",opts?opts->volume:1);r_q_number("pitch",opts?opts->pitch:1);r_append(",\"loop\":");r_append(opts&&opts->loop?"true":"false");RQueryResult result=r_query_end(q);RusticAudioVoice v={{0}};if(result.value.type!=RUSTIC_STRING)r_fail("Rustic C: invalid voice");snprintf(v.id,sizeof v.id,"%s",result.value.string);return v;}
static void r_audio_loaded_control(RusticLoadedAudio h,const char *op){RQuerySave q=r_query_begin(op);r_q_string("entity",h.id);(void)r_query_end(q);}
static void r_audio_loaded_stop(RusticLoadedAudio h){r_audio_loaded_control(h,"audio_loaded_stop");}
static void r_audio_loaded_pause(RusticLoadedAudio h){r_audio_loaded_control(h,"audio_loaded_pause");}
static void r_audio_loaded_resume(RusticLoadedAudio h){r_audio_loaded_control(h,"audio_loaded_resume");}
static void r_audio_unload(RusticLoadedAudio *h){if(h->id[0]){r_audio_loaded_control(*h,"audio_unload");h->id[0]=0;}}
static const struct {RusticAudioVoice(*play)(RusticLoadedAudio,const RusticAudioOptions*);void(*stop)(RusticLoadedAudio),(*pause)(RusticLoadedAudio),(*resume)(RusticLoadedAudio);void(*unload)(RusticLoadedAudio*);} LoadedAudio={r_audio_loaded_play,r_audio_loaded_stop,r_audio_loaded_pause,r_audio_loaded_resume,r_audio_unload};
static RusticAudioDevices r_audio_get_output_devices(void){RQuerySave s=r_query_begin("audio_get_output_devices");return r_query_end(s).devices;}
static RusticAudioDevices r_audio_get_input_devices(void){RQuerySave s=r_query_begin("audio_get_input_devices");return r_query_end(s).devices;}
static void r_audio_set_output_device(const char *device){RQuerySave s=r_query_begin("audio_set_output_device");r_q_string("device",device);(void)r_query_end(s);}
static void r_audio_set_input_device(const char *device){RQuerySave s=r_query_begin("audio_set_input_device");r_q_string("device",device);(void)r_query_end(s);}
static RusticAudioVoice r_audio_play_at(const char *source,RusticVector3 position,bool spatial,const RusticAudioOptions *opts){RQuerySave s=r_query_begin("audio_play");r_q_string("source",source);r_q_number("volume",opts?opts->volume:1);r_q_number("pitch",opts?opts->pitch:1);r_append(",\"loop\":");r_append(opts&&opts->loop?"true":"false");if(spatial)r_q_value("position",r_vec(position));RQueryResult result=r_query_end(s);RusticAudioVoice voice={{0}};if(result.value.type!=RUSTIC_STRING)r_fail("Rustic C: audio host returned an invalid voice");snprintf(voice.id,sizeof voice.id,"%s",result.value.string);return voice;}
static RusticAudioVoice r_audio_play(const char *source,const RusticAudioOptions *opts){return r_audio_play_at(source,(RusticVector3){0},false,opts);}
static RusticAudioVoice r_audio_at(const char *source,RusticVector3 position,const RusticAudioOptions *opts){return r_audio_play_at(source,position,true,opts);}
static void r_audio_control(RusticAudioVoice voice,const char *op){RQuerySave s=r_query_begin(op);r_q_string("entity",voice.id);(void)r_query_end(s);}
static void r_audio_stop(RusticAudioVoice v){r_audio_control(v,"audio_stop");}static void r_audio_pause(RusticAudioVoice v){r_audio_control(v,"audio_pause");}static void r_audio_resume(RusticAudioVoice v){r_audio_control(v,"audio_resume");}
static void r_audio_property(RusticAudioVoice voice,const char *op,double value){RQuerySave s=r_query_begin(op);r_q_string("entity",voice.id);r_q_number("value",value);(void)r_query_end(s);}
static void r_audio_volume(RusticAudioVoice v,double value){r_audio_property(v,"audio_volume",value);}static void r_audio_pitch(RusticAudioVoice v,double value){r_audio_property(v,"audio_pitch",value);}
static RusticAction r_action_from_zero(RusticAction a){RActionSave s=r_action_begin();r_append(a.private_data);if(r_written==0)r_fail("Rustic C: empty tween");r_output[--r_written]=0;r_q_number("from",0);r_append("}");RusticAction result=r_action_end(s);r_action_tokens(&result,&a);r_action_free(&a);return result;}
static RusticGameplayHandle r_audio_fade_in(RusticAudioVoice v,double duration,const char *ease,void(*fn)(void)){RusticAction a=r_action_tween(v.id,"Volume",(RusticValue){.type=RUSTIC_NUMBER,.number=1},duration,ease);return r_run_action(r_action_from_zero(a),fn);}
static RusticGameplayHandle r_audio_fade_out(RusticAudioVoice v,double duration,const char *ease,void(*fn)(void)){return r_tween_to(v.id,"Volume",(RusticValue){.type=RUSTIC_NUMBER,.number=0},duration,ease,fn);}
static RusticGameplayHandle r_audio_crossfade(RusticAudioVoice a,RusticAudioVoice b,double duration,const char *ease,void(*fn)(void)){RusticSequence s=r_sequence_new();r_sequence_add(&s,r_action_tween(a.id,"Volume",(RusticValue){.type=RUSTIC_NUMBER,.number=0},duration,ease));r_sequence_add(&s,r_action_from_zero(r_action_tween(b.id,"Volume",(RusticValue){.type=RUSTIC_NUMBER,.number=1},duration,ease)));RusticGameplayHandle h=r_run_action(r_sequence_action(&s,"parallel"),fn);r_sequence_destroy(&s);return h;}
typedef struct {double time;RusticValue value;const char *easing;} RusticKeyframe;
typedef struct {const char *target;const RusticKeyframe *keys;size_t count;} RusticAnimationTrack;
typedef struct {double time;const char *name;} RusticAnimationMarker;
typedef struct {const char *name;double duration;const RusticAnimationTrack *tracks;size_t trackCount;const RusticAnimationMarker *markers;size_t markerCount;} RusticAnimationClip;
typedef struct {double speed,blendIn,blendOut,weight;const char *easing,*blendInEase,*blendOutEase,*progressionEase;bool loop,additive;const char **mask;size_t maskCount;} RusticAnimationOptions;
static RusticAnimationOptions r_animation_defaults(void){return (RusticAnimationOptions){.speed=1,.weight=1,.easing="Linear"};}
static RusticList r_animation_load(const char *e,const char *source){const char *id=r_resolve(e);RQuerySave s=r_query_begin("animation_load");r_q_string("entity",id);r_q_string("source",source);return r_query_end(s).list;}
static RusticList r_animation_refs(const char *e){const char *id=r_resolve(e);RQuerySave s=r_query_begin("animation_list");r_q_string("entity",id);return r_query_end(s).list;}
static void r_animation_register(const char *e,RusticAnimationClip clip){const char *id=r_resolve(e);RQuerySave s=r_query_begin("animation_register");r_q_string("entity",id);r_append(",\"clip\":{\"name\":");r_quote(clip.name);r_q_number("duration",clip.duration);r_append(",\"tracks\":[");for(size_t i=0;i<clip.trackCount;i++){if(i)r_append(",");const RusticAnimationTrack *t=&clip.tracks[i];r_append("{\"target\":");r_quote(t->target);r_append(",\"keys\":[");for(size_t k=0;k<t->count;k++){if(k)r_append(",");r_append("{\"time\":");r_double(t->keys[k].time);r_q_value("value",t->keys[k].value);r_q_string("easing",t->keys[k].easing?t->keys[k].easing:"Linear");r_append("}");}r_append("]}");}r_append("],\"markers\":[");for(size_t i=0;i<clip.markerCount;i++){if(i)r_append(",");r_append("{\"time\":");r_double(clip.markers[i].time);r_q_string("name",clip.markers[i].name);r_append("}");}r_append("]}");(void)r_query_end(s);}
static RusticAction r_animation_action(const char *e,const char *name,const RusticAnimationOptions *provided){const char *id=r_resolve(e);RusticAnimationOptions opts=provided?*provided:r_animation_defaults();RQuerySave query=r_query_begin("animation_ref");r_q_string("entity",id);r_q_string("name",name);RQueryResult result=r_query_end(query);
    RActionSave s=r_action_begin();r_append("{\"kind\":\"animation_ref\"");r_q_string("entity",id);r_append(",\"clip\":");r_append(result.json);r_append(",\"options\":{\"speed\":");r_double(opts.speed);r_q_number("blend_in",opts.blendIn);r_q_number("blend_out",opts.blendOut);r_q_number("weight",opts.weight);r_q_string("blend_in_ease",opts.blendInEase?opts.blendInEase:opts.easing?opts.easing:"Linear");r_q_string("blend_out_ease",opts.blendOutEase?opts.blendOutEase:opts.easing?opts.easing:"Linear");r_q_string("progression_ease",opts.progressionEase);r_append(",\"additive\":");r_append(opts.additive?"true":"false");r_append(",\"layered\":");r_append(opts.maskCount?"true":"false");r_append("},\"mask\":[");for(size_t i=0;i<opts.maskCount;i++){if(i)r_append(",");r_quote(opts.mask[i]);}r_append("]}");return r_action_end(s);}
static RusticGameplayHandle r_animation_play(const char *e,const char *name,const RusticAnimationOptions *opts,void(*fn)(void)){RusticAction a=r_animation_action(e,name,opts);RusticGameplayHandle marker=r_gameplay_new(NULL);for(size_t i=0;i<4096;i++)if(r_gameplay_slots[i].id==marker){r_gameplay_slots[i].arguments=r_marker_dispatch;r_gameplay_slots[i].persistent=true;}
    RActionSave s=r_action_begin();r_append(a.private_data);r_output[--r_written]=0;r_append(",\"marker_token\":");r_gameplay_handle(marker);r_append("}");RusticAction withMarker=r_action_end(s);withMarker.private_tokens[withMarker.private_count++]=marker;r_action_free(&a);RusticGameplayHandle h=r_start_action(withMarker,opts&&opts->loop,0,false,fn);r_action_free(&withMarker);return h;}
static RusticGameplayHandle r_animation_value(const RusticKeyframe *keys,size_t count,void(*sample)(RusticValue),void(*finished)(void)){
    RusticGameplayHandle token=r_gameplay_new(NULL);for(size_t i=0;i<4096;i++)if(r_gameplay_slots[i].id==token){r_gameplay_slots[i].value=sample;r_gameplay_slots[i].persistent=true;}
    RQuerySave q=r_query_begin("keyframes");r_append(",\"token\":");r_gameplay_handle(token);r_append(",\"keys\":[");
    for(size_t i=0;i<count;i++){if(i)r_append(",");r_append("{\"time\":");r_double(keys[i].time);r_q_value("value",keys[i].value);r_q_string("easing",keys[i].easing?keys[i].easing:"Linear");r_append("}");}r_append("]");RQueryResult result=r_query_end(q);
    RActionSave s=r_action_begin();r_append(result.json);RusticAction action=r_action_end(s);action.private_tokens[action.private_count++]=token;return r_run_action(action,finished);
}
static void r_animation_add_marker(const char *e,const char *clip,double time,const char *name){const char *id=r_resolve(e);RQuerySave q=r_query_begin("animation_marker");r_q_string("entity",id);r_q_string("clip",clip);r_q_number("time",time);r_q_string("name",name);(void)r_query_end(q);}
static void r_animation_marker(RusticGameplayHandle owner,const char *name,void(*fn)(void)){if(!name||strlen(name)>256)r_fail("Rustic C: invalid marker name");for(size_t i=0;i<4096;i++)if(!r_marker_slots[i].owner){r_marker_slots[i].owner=owner;snprintf(r_marker_slots[i].name,sizeof r_marker_slots[i].name,"%s",name);r_marker_slots[i].fn=fn;return;}r_fail("Rustic C: marker subscription limit");}
static void r_gameplay_speed(RusticGameplayHandle h,double speed){r_command("gameplay");r_append(",\"request\":{\"command\":\"speed\",\"handle\":");r_gameplay_handle(h);r_q_number("speed",speed);r_append("}}");}
static void r_gameplay_loop(RusticGameplayHandle h,bool looping){r_command("gameplay");r_append(",\"request\":{\"command\":\"loop\",\"handle\":");r_gameplay_handle(h);r_append(",\"looping\":");r_append(looping?"true":"false");r_append("}}");}
static RusticOperationState r_gameplay_state(RusticGameplayHandle h){RQuerySave q=r_query_begin("operation_state");r_append(",\"handle\":");r_gameplay_handle(h);return r_query_end(q).operation;}
static void r_gameplay_finished(RusticGameplayHandle h,void(*fn)(void)){for(size_t i=0;i<4096;i++)if(r_gameplay_slots[i].id==h){r_gameplay_slots[i].finished=fn;return;}r_fail("Rustic C: unknown action handle");}
static RusticSequence *r_sequence_animation(RusticSequence *s,const char *e,const char *name,const RusticAnimationOptions *opts){return r_sequence_add(s,r_animation_action(e,name,opts));}
static RusticGameplayHandle r_animation_blend(const char *e,const char *from,const char *to,double duration,const char *ease,void(*fn)(void)){RusticAnimationOptions opts=r_animation_defaults();opts.blendIn=duration;opts.easing=ease;RusticAction a=r_animation_action(e,to,&opts);const char *id=r_resolve(e);RQuerySave q=r_query_begin("animation_ref");r_q_string("entity",id);r_q_string("name",from);RQueryResult source=r_query_end(q);RActionSave s=r_action_begin();r_append(a.private_data);r_output[--r_written]=0;r_append(",\"blend_source\":");r_append(source.json);r_append("}");RusticAction result=r_action_end(s);r_action_free(&a);return r_run_action(result,fn);}
static RusticGameplayHandle r_animation_ik(const char *root,const char *middle,const char *tip,RusticVector3 target,RusticVector3 pole,double duration,double weight,const char *ease,void(*fn)(void)){const char *r=r_resolve(root),*m=r_resolve(middle),*t=r_resolve(tip);RQuerySave q=r_query_begin("animation_ik");r_q_string("root",r);r_q_string("middle",m);r_q_string("tip",t);r_q_value("target",r_vec(target));r_q_value("pole",r_vec(pole));r_q_number("duration",duration);r_q_number("weight",weight);r_q_string("easing",ease?ease:"Linear");RQueryResult result=r_query_end(q);RActionSave s=r_action_begin();r_append(result.json);return r_run_action(r_action_end(s),fn);}
static RusticGameplayHandle r_recoil(const char *e,RusticValue rotation,double d,const char *ease,void(*fn)(void)){RusticAction a=r_action_tween(e,"Rotation",rotation,d,ease);RusticGameplayHandle h=r_start_action(a,false,1,true,fn);r_action_free(&a);return h;}
typedef RusticGameplayHandle RusticConnection;
static RusticConnection r_events_connect(const char *source,const char *name,void(*fn)(const RusticValue*,size_t),bool once){const char *id=source?r_resolve(source):NULL;RusticGameplayHandle token=r_gameplay_new(NULL);for(size_t i=0;i<4096;i++)if(r_gameplay_slots[i].id==token){r_gameplay_slots[i].arguments=fn;r_gameplay_slots[i].persistent=!once;r_gameplay_slots[i].signal=true;}
    r_command("gameplay");r_append(",\"request\":{\"command\":\"connect\",\"token\":");r_gameplay_handle(token);r_append(",\"signal\":{\"name\":");r_quote(name);r_q_string("source",id);r_append("},\"once\":");r_append(once?"true":"false");r_append("}}");return token;}
static RusticConnection r_events_on(const char *name,void(*fn)(const RusticValue*,size_t)){return r_events_connect(NULL,name,fn,false);}static RusticConnection r_events_once(const char *name,void(*fn)(const RusticValue*,size_t)){return r_events_connect(NULL,name,fn,true);}
static void r_events_disconnect(RusticConnection token){r_command("gameplay");r_append(",\"request\":{\"command\":\"disconnect\",\"token\":");r_gameplay_handle(token);r_append("}}");for(size_t i=0;i<4096;i++)if(r_gameplay_slots[i].id==token)r_gameplay_slots[i].id=0;}
static void r_events_emit(const char *name,const RusticValue *arguments,size_t count,const char *source){const char *id=source?r_resolve(source):NULL;r_command("gameplay");r_append(",\"request\":{\"command\":\"emit\",\"signal\":{\"name\":");r_quote(name);r_q_string("source",id);r_append("},\"arguments\":[");for(size_t i=0;i<count;i++){if(i)r_append(",");r_write_value(arguments[i]);}r_append("]}}");}
static RusticGameplayHandle r_tween_value(RusticValue from,RusticValue to,double duration,const char *ease,void(*sample)(RusticValue),void(*finished)(void)){RusticGameplayHandle token=r_gameplay_new(NULL);for(size_t i=0;i<4096;i++)if(r_gameplay_slots[i].id==token){r_gameplay_slots[i].value=sample;r_gameplay_slots[i].persistent=true;}RActionSave s=r_action_begin();r_append("{\"kind\":\"value\",\"from\":");r_write_value(from);r_q_value("to",to);r_q_number("duration",duration);r_q_string("easing",ease?ease:"Linear");r_append(",\"token\":");r_gameplay_handle(token);r_append("}");RusticAction a=r_action_end(s);a.private_tokens[a.private_count++]=token;return r_run_action(a,finished);}
static void r_clock_scale(double value){RQuerySave s=r_query_begin("clock");r_q_number("scale",value);(void)r_query_end(s);}
static void r_clock_pause(void){RQuerySave s=r_query_begin("clock");r_append(",\"paused\":true");(void)r_query_end(s);}
static void r_clock_resume(void){RQuerySave s=r_query_begin("clock");r_append(",\"paused\":false");(void)r_query_end(s);}
static RusticClockState r_clock_state(void){RQuerySave q=r_query_begin("clock");return r_query_end(q).clock;}
static const struct{void(*timeScale)(double),(*pause)(void),(*resume)(void);RusticClockState(*state)(void);} Clock={r_clock_scale,r_clock_pause,r_clock_resume,r_clock_state};
static const struct {RusticAudioVoice(*play)(const char*,const RusticAudioOptions*),(*playAt)(const char*,RusticVector3,const RusticAudioOptions*);void(*stop)(RusticAudioVoice),(*pause)(RusticAudioVoice),(*resume)(RusticAudioVoice),(*volume)(RusticAudioVoice,double),(*pitch)(RusticAudioVoice,double);RusticGameplayHandle(*fadeIn)(RusticAudioVoice,double,const char*,void(*)(void)),(*fadeOut)(RusticAudioVoice,double,const char*,void(*)(void)),(*crossfade)(RusticAudioVoice,RusticAudioVoice,double,const char*,void(*)(void));RusticAudioDevices(*get_output_devices)(void),(*get_input_devices)(void);void(*set_output_device)(const char*),(*set_input_device)(const char*);RusticLoadedAudio(*load)(const char*);} Audio={r_audio_play,r_audio_at,r_audio_stop,r_audio_pause,r_audio_resume,r_audio_volume,r_audio_pitch,r_audio_fade_in,r_audio_fade_out,r_audio_crossfade,r_audio_get_output_devices,r_audio_get_input_devices,r_audio_set_output_device,r_audio_set_input_device,r_audio_load};
static const struct {void(*addMarker)(const char*,const char*,double,const char*);RusticGameplayHandle(*value)(const RusticKeyframe*,size_t,void(*)(RusticValue),void(*)(void));RusticAnimationOptions(*defaults)(void);RusticList(*load)(const char*,const char*),(*clips)(const char*);void(*registerClip)(const char*,RusticAnimationClip);RusticGameplayHandle(*play)(const char*,const char*,const RusticAnimationOptions*,void(*)(void));void(*onMarker)(RusticGameplayHandle,const char*,void(*)(void)),(*stop)(RusticGameplayHandle),(*pause)(RusticGameplayHandle),(*speed)(RusticGameplayHandle,double),(*loop)(RusticGameplayHandle,bool);RusticGameplayHandle(*blend)(const char*,const char*,const char*,double,const char*,void(*)(void)),(*transition)(const char*,const char*,const char*,double,const char*,void(*)(void)),(*ik)(const char*,const char*,const char*,RusticVector3,RusticVector3,double,double,const char*,void(*)(void)),(*footPlacement)(const char*,const char*,const char*,RusticVector3,RusticVector3,double,double,const char*,void(*)(void)),(*lookAt)(const char*,RusticVector3,double,const char*,void(*)(void)),(*headTracking)(const char*,RusticVector3,double,const char*,void(*)(void)),(*recoil)(const char*,RusticValue,double,const char*,void(*)(void));} Animation={r_animation_add_marker,r_animation_value,r_animation_defaults,r_animation_load,r_animation_refs,r_animation_register,r_animation_play,r_animation_marker,r_gameplay_cancel,r_gameplay_pause,r_gameplay_speed,r_gameplay_loop,r_animation_blend,r_animation_blend,r_animation_ik,r_animation_ik,r_look_at,r_look_at,r_recoil};
static const struct {RusticConnection(*on)(const char*,void(*)(const RusticValue*,size_t)),(*once)(const char*,void(*)(const RusticValue*,size_t)),(*connect)(const char*,const char*,void(*)(const RusticValue*,size_t),bool);void(*emit)(const char*,const RusticValue*,size_t,const char*),(*disconnect)(RusticConnection);} Events={r_events_on,r_events_once,r_events_connect,r_events_emit,r_events_disconnect};
typedef struct {double duration,speed;bool useSpeed;const char *easing;} RusticMotionOptions;
static RusticGameplayHandle r_tween_options(const char *e,const char *p,RusticValue to,RusticMotionOptions opts,void(*fn)(void)){const char *id=r_resolve(e);RActionSave s=r_action_begin();r_append("{\"kind\":\"tween\",\"target\":{\"entity\":");r_quote(id);r_q_string("property",p);r_append("}");r_q_value("to",to);r_q_number(opts.useSpeed?"speed":"duration",opts.useSpeed?opts.speed:opts.duration);r_q_string("easing",opts.easing?opts.easing:"Linear");r_append("}");return r_run_action(r_action_end(s),fn);}
static RusticGameplayHandle r_move_options(const char *e,RusticVector3 p,RusticMotionOptions opts,void(*fn)(void)){return r_tween_options(e,"Position",r_vec(p),opts,fn);}
static const struct {RusticGameplayHandle (*to)(const char*,const char*,RusticValue,double,const char*,void(*)(void));RusticGameplayHandle (*move)(const char*,RusticVector3,double,const char*,void(*)(void));RusticGameplayHandle(*rotate)(const char*,RusticValue,double,const char*,void(*)(void)),(*scale)(const char*,RusticVector3,double,const char*,void(*)(void));RusticAction(*action)(const char*,const char*,RusticValue,double,const char*);RusticGameplayHandle(*value)(RusticValue,RusticValue,double,const char*,void(*)(RusticValue),void(*)(void));RusticGameplayHandle(*toOptions)(const char*,const char*,RusticValue,RusticMotionOptions,void(*)(void));} Tween={r_tween_to,r_tween_move,r_rotate_to,r_scale_to,r_action_tween,r_tween_value,r_tween_options};
static const char *r_camera_current(void){RQuerySave q=r_query_begin("camera_current");return r_query_end(q).value.string;}
static const struct {RusticGameplayHandle(*moveTo)(const char*,RusticVector3,double,const char*,void(*)(void)),(*move)(const char*,RusticVector3,double,const char*,void(*)(void)),(*rotateTo)(const char*,RusticValue,double,const char*,void(*)(void)),(*lookAt)(const char*,RusticVector3,double,const char*,void(*)(void)),(*follow)(const char*,const char*,RusticVector3,double,const char*,void(*)(void)),(*orbit)(const char*,RusticVector3,double,double,double,const char*,void(*)(void));RusticGameplayHandle(*moveToOptions)(const char*,RusticVector3,RusticMotionOptions,void(*)(void));} Movement={r_tween_move,r_move_relative,r_rotate_to,r_look_at,r_follow,r_orbit,r_move_options};
static const struct {RusticGameplayHandle (*after)(double,void(*)(void)),(*every)(double,void(*)(void),unsigned);} Timer={r_timer_after,r_timer_every};
#define R_SEQUENCE_API {r_sequence_new,r_sequence_to,r_sequence_move,r_sequence_wait,r_sequence_call,r_sequence_parallel,r_sequence_play,r_sequence_destroy,r_sequence_animation}
static const struct {RusticSequence(*create)(void);RusticSequence*(*to)(RusticSequence*,const char*,const char*,RusticValue,double,const char*),*(*move)(RusticSequence*,const char*,RusticVector3,double,const char*),*(*wait)(RusticSequence*,double),*(*call)(RusticSequence*,void(*)(void)),*(*parallel)(RusticSequence*,RusticSequence*,size_t);RusticGameplayHandle(*play)(RusticSequence*,void(*)(void));void(*destroy)(RusticSequence*);RusticSequence*(*animation)(RusticSequence*,const char*,const char*,const RusticAnimationOptions*);} Sequence=R_SEQUENCE_API,Timeline=R_SEQUENCE_API;
static const struct {RusticPath(*create)(const RusticPathPoint*,size_t,const char*);RusticGameplayHandle(*follow)(const char*,RusticPath,double,double,const char*,bool,bool,bool,void(*)(void));void(*destroy)(RusticPath*);} Path={r_path_create,r_path_follow,r_path_destroy};
static const struct {RusticGameplayHandle(*fade)(const char*,double,double,const char*,void(*)(void)),(*flash)(const char*,RusticVector3,double,const char*,void(*)(void)),(*pulse)(const char*,RusticVector3,double,const char*,void(*)(void)),(*shake)(const char*,double,double,const char*,void(*)(void));} Effects={r_fade,r_flash,r_pulse,r_shake};
static const struct {RusticGameplayHandle(*moveTo)(const char*,RusticVector3,double,const char*,void(*)(void)),(*zoom)(const char*,double,double,const char*,void(*)(void)),(*fov)(const char*,double,double,const char*,void(*)(void)),(*follow)(const char*,const char*,RusticVector3,double,const char*,void(*)(void)),(*lookAt)(const char*,RusticVector3,double,const char*,void(*)(void)),(*orbit)(const char*,RusticVector3,double,double,double,const char*,void(*)(void)),(*shake)(const char*,double,double,const char*,void(*)(void)),(*transition)(const char*,RusticVector3,RusticValue,double,double,const char*,void(*)(void));const char *(*current)(void);} Camera={r_tween_move,r_camera_zoom,r_camera_zoom,r_follow,r_look_at,r_orbit,r_shake,r_camera_transition,r_camera_current};
static const struct {RusticOperationState(*state)(RusticGameplayHandle);void (*pause)(RusticGameplayHandle),(*resume)(RusticGameplayHandle),(*cancel)(RusticGameplayHandle),(*reverse)(RusticGameplayHandle),(*onFinished)(RusticGameplayHandle,void(*)(void)),(*speed)(RusticGameplayHandle,double),(*loop)(RusticGameplayHandle,bool);} Gameplay={r_gameplay_state,r_gameplay_pause,r_gameplay_resume,r_gameplay_cancel,r_gameplay_reverse,r_gameplay_finished,r_gameplay_speed,r_gameplay_loop};
static const struct {const char *Linear,*InSine,*OutSine,*InOutSine,*InQuad,*OutQuad,*InOutQuad,*InCubic,*OutCubic,*InOutCubic,*InQuart,*OutQuart,*InOutQuart,*InQuint,*OutQuint,*InOutQuint,*InExpo,*OutExpo,*InOutExpo,*InCirc,*OutCirc,*InOutCirc,*InBack,*OutBack,*InOutBack,*InElastic,*OutElastic,*InOutElastic,*InBounce,*OutBounce,*InOutBounce;} Ease={"Linear","InSine","OutSine","InOutSine","InQuad","OutQuad","InOutQuad","InCubic","OutCubic","InOutCubic","InQuart","OutQuart","InOutQuart","InQuint","OutQuint","InOutQuint","InExpo","OutExpo","InOutExpo","InCirc","OutCirc","InOutCirc","InBack","OutBack","InOutBack","InElastic","OutElastic","InOutElastic","InBounce","OutBounce","InOutBounce"};

/* Canonical API paths. C passes self explicitly. */
typedef struct RusticScenePath RusticScenePath;
struct RusticScenePath {char source[4096];void(*clone)(RusticScenePath,const char*);void(*add)(RusticScenePath,const char*);void(*setCurrentCamera)(RusticScenePath);void(*emit)(RusticScenePath,const char*,const RusticValue*,size_t);RusticGameplayHandle(*tweenTo)(RusticScenePath self,const char *property,RusticValue to,double duration,const char *easing,void (*finished)(void));RusticGameplayHandle(*tweenMove)(RusticScenePath self,RusticVector3 p,double duration,const char *easing,void (*finished)(void));RusticGameplayHandle(*tweenRotate)(RusticScenePath self,RusticValue q,double d,const char *ease,void(*fn)(void));RusticGameplayHandle(*tweenScale)(RusticScenePath self,RusticVector3 scale,double d,const char *ease,void(*fn)(void));RusticAction(*tweenAction)(RusticScenePath self,const char *property,RusticValue to,double duration,const char *easing);RusticGameplayHandle(*tweenToOptions)(RusticScenePath self,const char *p,RusticValue to,RusticMotionOptions opts,void(*fn)(void));RusticGameplayHandle(*movementMoveTo)(RusticScenePath self,RusticVector3 p,double duration,const char *easing,void (*finished)(void));RusticGameplayHandle(*movementMove)(RusticScenePath self,RusticVector3 offset,double d,const char *ease,void(*fn)(void));RusticGameplayHandle(*movementRotateTo)(RusticScenePath self,RusticValue q,double d,const char *ease,void(*fn)(void));RusticGameplayHandle(*movementLookAt)(RusticScenePath self,RusticVector3 target,double d,const char *ease,void(*fn)(void));RusticGameplayHandle(*movementFollow)(RusticScenePath self,const char *target,RusticVector3 offset,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*movementOrbit)(RusticScenePath self,RusticVector3 center,double radius,double turns,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*movementMoveToOptions)(RusticScenePath self,RusticVector3 p,RusticMotionOptions opts,void(*fn)(void));RusticGameplayHandle(*pathFollow)(RusticScenePath self,RusticPath path,double duration,double speed,const char *ease,bool loop,bool pingPong,bool orient,void(*fn)(void));RusticGameplayHandle(*effectsFade)(RusticScenePath self,double opacity,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*effectsFlash)(RusticScenePath self,RusticVector3 color,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*effectsShake)(RusticScenePath self,double strength,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*effectsPulse)(RusticScenePath self,RusticVector3 scale,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*cameraMoveTo)(RusticScenePath self,RusticVector3 p,double duration,const char *easing,void (*finished)(void));RusticGameplayHandle(*cameraZoom)(RusticScenePath self,double fov,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*cameraFov)(RusticScenePath self,double fov,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*cameraFollow)(RusticScenePath self,const char *target,RusticVector3 offset,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*cameraOrbit)(RusticScenePath self,RusticVector3 center,double radius,double turns,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*cameraLookAt)(RusticScenePath self,RusticVector3 target,double d,const char *ease,void(*fn)(void));RusticGameplayHandle(*cameraShake)(RusticScenePath self,double strength,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*cameraTransition)(RusticScenePath self,RusticVector3 p,RusticValue rotation,double fov,double duration,const char *ease,void(*fn)(void));void(*physicsImpulse)(RusticScenePath self,RusticVector3 v);void(*physicsForce)(RusticScenePath self,RusticVector3 v,double delta);void(*physicsLaunch)(RusticScenePath self,RusticVector3 v);void(*physicsKnockback)(RusticScenePath self,RusticVector3 v);void(*animationRegisterClip)(RusticScenePath self,RusticAnimationClip clip);RusticList(*animationClips)(RusticScenePath self);RusticGameplayHandle(*animationPlay)(RusticScenePath self,const char *name,const RusticAnimationOptions *opts,void(*fn)(void));RusticGameplayHandle(*animationBlend)(RusticScenePath self,const char *from,const char *to,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*animationTransition)(RusticScenePath self,const char *from,const char *to,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*animationIk)(RusticScenePath self,const char *middle,const char *tip,RusticVector3 target,RusticVector3 pole,double duration,double weight,const char *ease,void(*fn)(void));RusticGameplayHandle(*animationFootPlacement)(RusticScenePath self,const char *middle,const char *tip,RusticVector3 target,RusticVector3 pole,double duration,double weight,const char *ease,void(*fn)(void));RusticGameplayHandle(*animationLookAt)(RusticScenePath self,RusticVector3 target,double d,const char *ease,void(*fn)(void));RusticGameplayHandle(*animationHeadTracking)(RusticScenePath self,RusticVector3 target,double d,const char *ease,void(*fn)(void));RusticGameplayHandle(*animationRecoil)(RusticScenePath self,RusticValue rotation,double d,const char *ease,void(*fn)(void));void(*animationAddMarker)(RusticScenePath self,const char *clip,double time,const char *name);RusticConnection(*eventsConnect)(RusticScenePath self,const char *name,void(*fn)(const RusticValue*,size_t),bool once);bool(*tagsAdd)(RusticScenePath self,const char* name);bool(*tagsRemove)(RusticScenePath self,const char* name);bool(*groupsAdd)(RusticScenePath self,const char* name);void(*hierarchySetParent)(RusticScenePath self,const char* parent,bool keep_world);RusticList(*hierarchyGetChildren)(RusticScenePath self);void(*hierarchyDetach)(RusticScenePath self,bool keep_world);void(*entityDestroy)(RusticScenePath self);void(*entityDestroyAfter)(RusticScenePath self,double delay);bool(*entityIsValid)(RusticScenePath self);bool(*componentsHas)(RusticScenePath self,const char* name);RusticList(*componentsGetAll)(RusticScenePath self);RusticList(*componentsFindInChildren)(RusticScenePath self,const char* name);RusticGameplayHandle(*tween)(RusticScenePath self,const char *property,RusticValue to,double duration,const char *easing,void (*finished)(void));RusticGameplayHandle(*moveTo)(RusticScenePath self,RusticVector3 p,double duration,const char *easing,void (*finished)(void));RusticGameplayHandle(*move)(RusticScenePath self,RusticVector3 offset,double d,const char *ease,void(*fn)(void));RusticGameplayHandle(*rotateTo)(RusticScenePath self,RusticValue q,double d,const char *ease,void(*fn)(void));RusticGameplayHandle(*lookAt)(RusticScenePath self,RusticVector3 target,double d,const char *ease,void(*fn)(void));RusticGameplayHandle(*follow)(RusticScenePath self,const char *target,RusticVector3 offset,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*orbit)(RusticScenePath self,RusticVector3 center,double radius,double turns,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*fade)(RusticScenePath self,double opacity,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*flash)(RusticScenePath self,RusticVector3 color,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*shake)(RusticScenePath self,double strength,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*pulse)(RusticScenePath self,RusticVector3 scale,double duration,const char *ease,void(*fn)(void));void(*impulse)(RusticScenePath self,RusticVector3 v);void(*force)(RusticScenePath self,RusticVector3 v,double delta);void(*launch)(RusticScenePath self,RusticVector3 v);void(*knockback)(RusticScenePath self,RusticVector3 v);RusticGameplayHandle(*playAnimation)(RusticScenePath self,const char *name,const RusticAnimationOptions *opts,void(*fn)(void));void(*destroy)(RusticScenePath self);void(*destroyAfter)(RusticScenePath self,double delay);bool(*isValid)(RusticScenePath self);bool(*addTag)(RusticScenePath self,const char* name);bool(*removeTag)(RusticScenePath self,const char* name);void(*setParent)(RusticScenePath self,const char* parent,bool keep_world);RusticList(*getChildren)(RusticScenePath self);void(*detach)(RusticScenePath self,bool keep_world);bool(*hasComponent)(RusticScenePath self,const char* name);RusticList(*getComponents)(RusticScenePath self);RusticList(*findInChildren)(RusticScenePath self,const char* name);RusticGameplayHandle(*followPath)(RusticScenePath self,RusticPath path,double duration,double speed,const char *ease,bool loop,bool pingPong,bool orient,void(*fn)(void));void(*EditAttribute)(RusticScenePath,const char*,RusticValue);};
static RusticGameplayHandle r_path_scene_tweenTo(RusticScenePath self,const char *property,RusticValue to,double duration,const char *easing,void (*finished)(void)){return r_tween_to(self.source,property,to,duration,easing,finished);}
static RusticGameplayHandle r_path_scene_tweenMove(RusticScenePath self,RusticVector3 p,double duration,const char *easing,void (*finished)(void)){return r_tween_move(self.source,p,duration,easing,finished);}
static RusticGameplayHandle r_path_scene_tweenRotate(RusticScenePath self,RusticValue q,double d,const char *ease,void(*fn)(void)){return r_rotate_to(self.source,q,d,ease,fn);}
static RusticGameplayHandle r_path_scene_tweenScale(RusticScenePath self,RusticVector3 scale,double d,const char *ease,void(*fn)(void)){return r_scale_to(self.source,scale,d,ease,fn);}
static RusticAction r_path_scene_tweenAction(RusticScenePath self,const char *property,RusticValue to,double duration,const char *easing){return r_action_tween(self.source,property,to,duration,easing);}
static RusticGameplayHandle r_path_scene_tweenToOptions(RusticScenePath self,const char *p,RusticValue to,RusticMotionOptions opts,void(*fn)(void)){return r_tween_options(self.source,p,to,opts,fn);}
static RusticGameplayHandle r_path_scene_movementMoveTo(RusticScenePath self,RusticVector3 p,double duration,const char *easing,void (*finished)(void)){return r_tween_move(self.source,p,duration,easing,finished);}
static RusticGameplayHandle r_path_scene_movementMove(RusticScenePath self,RusticVector3 offset,double d,const char *ease,void(*fn)(void)){return r_move_relative(self.source,offset,d,ease,fn);}
static RusticGameplayHandle r_path_scene_movementRotateTo(RusticScenePath self,RusticValue q,double d,const char *ease,void(*fn)(void)){return r_rotate_to(self.source,q,d,ease,fn);}
static RusticGameplayHandle r_path_scene_movementLookAt(RusticScenePath self,RusticVector3 target,double d,const char *ease,void(*fn)(void)){return r_look_at(self.source,target,d,ease,fn);}
static RusticGameplayHandle r_path_scene_movementFollow(RusticScenePath self,const char *target,RusticVector3 offset,double duration,const char *ease,void(*fn)(void)){return r_follow(self.source,target,offset,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_movementOrbit(RusticScenePath self,RusticVector3 center,double radius,double turns,double duration,const char *ease,void(*fn)(void)){return r_orbit(self.source,center,radius,turns,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_movementMoveToOptions(RusticScenePath self,RusticVector3 p,RusticMotionOptions opts,void(*fn)(void)){return r_move_options(self.source,p,opts,fn);}
static RusticGameplayHandle r_path_scene_pathFollow(RusticScenePath self,RusticPath path,double duration,double speed,const char *ease,bool loop,bool pingPong,bool orient,void(*fn)(void)){return r_path_follow(self.source,path,duration,speed,ease,loop,pingPong,orient,fn);}
static RusticGameplayHandle r_path_scene_effectsFade(RusticScenePath self,double opacity,double duration,const char *ease,void(*fn)(void)){return r_fade(self.source,opacity,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_effectsFlash(RusticScenePath self,RusticVector3 color,double duration,const char *ease,void(*fn)(void)){return r_flash(self.source,color,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_effectsShake(RusticScenePath self,double strength,double duration,const char *ease,void(*fn)(void)){return r_shake(self.source,strength,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_effectsPulse(RusticScenePath self,RusticVector3 scale,double duration,const char *ease,void(*fn)(void)){return r_pulse(self.source,scale,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_cameraMoveTo(RusticScenePath self,RusticVector3 p,double duration,const char *easing,void (*finished)(void)){return r_tween_move(self.source,p,duration,easing,finished);}
static RusticGameplayHandle r_path_scene_cameraZoom(RusticScenePath self,double fov,double duration,const char *ease,void(*fn)(void)){return r_camera_zoom(self.source,fov,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_cameraFov(RusticScenePath self,double fov,double duration,const char *ease,void(*fn)(void)){return r_camera_zoom(self.source,fov,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_cameraFollow(RusticScenePath self,const char *target,RusticVector3 offset,double duration,const char *ease,void(*fn)(void)){return r_follow(self.source,target,offset,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_cameraOrbit(RusticScenePath self,RusticVector3 center,double radius,double turns,double duration,const char *ease,void(*fn)(void)){return r_orbit(self.source,center,radius,turns,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_cameraLookAt(RusticScenePath self,RusticVector3 target,double d,const char *ease,void(*fn)(void)){return r_look_at(self.source,target,d,ease,fn);}
static RusticGameplayHandle r_path_scene_cameraShake(RusticScenePath self,double strength,double duration,const char *ease,void(*fn)(void)){return r_shake(self.source,strength,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_cameraTransition(RusticScenePath self,RusticVector3 p,RusticValue rotation,double fov,double duration,const char *ease,void(*fn)(void)){return r_camera_transition(self.source,p,rotation,fov,duration,ease,fn);}
static void r_path_scene_physicsImpulse(RusticScenePath self,RusticVector3 v){r_impulse(self.source,v);}
static void r_path_scene_physicsForce(RusticScenePath self,RusticVector3 v,double delta){r_force(self.source,v,delta);}
static void r_path_scene_physicsLaunch(RusticScenePath self,RusticVector3 v){r_launch(self.source,v);}
static void r_path_scene_physicsKnockback(RusticScenePath self,RusticVector3 v){r_impulse(self.source,v);}
static void r_path_scene_animationRegisterClip(RusticScenePath self,RusticAnimationClip clip){r_animation_register(self.source,clip);}
static RusticList r_path_scene_animationClips(RusticScenePath self){return r_animation_refs(self.source);}
static RusticGameplayHandle r_path_scene_animationPlay(RusticScenePath self,const char *name,const RusticAnimationOptions *opts,void(*fn)(void)){return r_animation_play(self.source,name,opts,fn);}
static RusticGameplayHandle r_path_scene_animationBlend(RusticScenePath self,const char *from,const char *to,double duration,const char *ease,void(*fn)(void)){return r_animation_blend(self.source,from,to,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_animationTransition(RusticScenePath self,const char *from,const char *to,double duration,const char *ease,void(*fn)(void)){return r_animation_blend(self.source,from,to,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_animationIk(RusticScenePath self,const char *middle,const char *tip,RusticVector3 target,RusticVector3 pole,double duration,double weight,const char *ease,void(*fn)(void)){return r_animation_ik(self.source,middle,tip,target,pole,duration,weight,ease,fn);}
static RusticGameplayHandle r_path_scene_animationFootPlacement(RusticScenePath self,const char *middle,const char *tip,RusticVector3 target,RusticVector3 pole,double duration,double weight,const char *ease,void(*fn)(void)){return r_animation_ik(self.source,middle,tip,target,pole,duration,weight,ease,fn);}
static RusticGameplayHandle r_path_scene_animationLookAt(RusticScenePath self,RusticVector3 target,double d,const char *ease,void(*fn)(void)){return r_look_at(self.source,target,d,ease,fn);}
static RusticGameplayHandle r_path_scene_animationHeadTracking(RusticScenePath self,RusticVector3 target,double d,const char *ease,void(*fn)(void)){return r_look_at(self.source,target,d,ease,fn);}
static RusticGameplayHandle r_path_scene_animationRecoil(RusticScenePath self,RusticValue rotation,double d,const char *ease,void(*fn)(void)){return r_recoil(self.source,rotation,d,ease,fn);}
static void r_path_scene_animationAddMarker(RusticScenePath self,const char *clip,double time,const char *name){r_animation_add_marker(self.source,clip,time,name);}
static RusticConnection r_path_scene_eventsConnect(RusticScenePath self,const char *name,void(*fn)(const RusticValue*,size_t),bool once){return r_events_connect(self.source,name,fn,once);}
static bool r_path_scene_tagsAdd(RusticScenePath self,const char* name){return r_tags_add(self.source,name);}
static bool r_path_scene_tagsRemove(RusticScenePath self,const char* name){return r_tags_remove(self.source,name);}
static bool r_path_scene_groupsAdd(RusticScenePath self,const char* name){return r_groups_add(name,self.source);}
static void r_path_scene_hierarchySetParent(RusticScenePath self,const char* parent,bool keep_world){r_hierarchy_set_parent(self.source,parent,keep_world);}
static RusticList r_path_scene_hierarchyGetChildren(RusticScenePath self){return r_hierarchy_get_children(self.source);}
static void r_path_scene_hierarchyDetach(RusticScenePath self,bool keep_world){r_hierarchy_detach(self.source,keep_world);}
static void r_path_scene_entityDestroy(RusticScenePath self){r_entity_destroy(self.source);}
static void r_path_scene_entityDestroyAfter(RusticScenePath self,double delay){r_entity_destroy_after(self.source,delay);}
static bool r_path_scene_entityIsValid(RusticScenePath self){return r_entity_is_valid(self.source);}
static bool r_path_scene_componentsHas(RusticScenePath self,const char* name){return r_components_has(self.source,name);}
static RusticList r_path_scene_componentsGetAll(RusticScenePath self){return r_components_get_all(self.source);}
static RusticList r_path_scene_componentsFindInChildren(RusticScenePath self,const char* name){return r_components_find_in_children(self.source,name);}
static RusticGameplayHandle r_path_scene_tween(RusticScenePath self,const char *property,RusticValue to,double duration,const char *easing,void (*finished)(void)){return r_tween_to(self.source,property,to,duration,easing,finished);}
static RusticGameplayHandle r_path_scene_moveTo(RusticScenePath self,RusticVector3 p,double duration,const char *easing,void (*finished)(void)){return r_tween_move(self.source,p,duration,easing,finished);}
static RusticGameplayHandle r_path_scene_move(RusticScenePath self,RusticVector3 offset,double d,const char *ease,void(*fn)(void)){return r_move_relative(self.source,offset,d,ease,fn);}
static RusticGameplayHandle r_path_scene_rotateTo(RusticScenePath self,RusticValue q,double d,const char *ease,void(*fn)(void)){return r_rotate_to(self.source,q,d,ease,fn);}
static RusticGameplayHandle r_path_scene_lookAt(RusticScenePath self,RusticVector3 target,double d,const char *ease,void(*fn)(void)){return r_look_at(self.source,target,d,ease,fn);}
static RusticGameplayHandle r_path_scene_follow(RusticScenePath self,const char *target,RusticVector3 offset,double duration,const char *ease,void(*fn)(void)){return r_follow(self.source,target,offset,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_orbit(RusticScenePath self,RusticVector3 center,double radius,double turns,double duration,const char *ease,void(*fn)(void)){return r_orbit(self.source,center,radius,turns,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_fade(RusticScenePath self,double opacity,double duration,const char *ease,void(*fn)(void)){return r_fade(self.source,opacity,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_flash(RusticScenePath self,RusticVector3 color,double duration,const char *ease,void(*fn)(void)){return r_flash(self.source,color,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_shake(RusticScenePath self,double strength,double duration,const char *ease,void(*fn)(void)){return r_shake(self.source,strength,duration,ease,fn);}
static RusticGameplayHandle r_path_scene_pulse(RusticScenePath self,RusticVector3 scale,double duration,const char *ease,void(*fn)(void)){return r_pulse(self.source,scale,duration,ease,fn);}
static void r_path_scene_impulse(RusticScenePath self,RusticVector3 v){r_impulse(self.source,v);}
static void r_path_scene_force(RusticScenePath self,RusticVector3 v,double delta){r_force(self.source,v,delta);}
static void r_path_scene_launch(RusticScenePath self,RusticVector3 v){r_launch(self.source,v);}
static void r_path_scene_knockback(RusticScenePath self,RusticVector3 v){r_impulse(self.source,v);}
static RusticGameplayHandle r_path_scene_playAnimation(RusticScenePath self,const char *name,const RusticAnimationOptions *opts,void(*fn)(void)){return r_animation_play(self.source,name,opts,fn);}
static void r_path_scene_destroy(RusticScenePath self){r_entity_destroy(self.source);}
static void r_path_scene_destroyAfter(RusticScenePath self,double delay){r_entity_destroy_after(self.source,delay);}
static bool r_path_scene_isValid(RusticScenePath self){return r_entity_is_valid(self.source);}
static bool r_path_scene_addTag(RusticScenePath self,const char* name){return r_tags_add(self.source,name);}
static bool r_path_scene_removeTag(RusticScenePath self,const char* name){return r_tags_remove(self.source,name);}
static void r_path_scene_setParent(RusticScenePath self,const char* parent,bool keep_world){r_hierarchy_set_parent(self.source,parent,keep_world);}
static RusticList r_path_scene_getChildren(RusticScenePath self){return r_hierarchy_get_children(self.source);}
static void r_path_scene_detach(RusticScenePath self,bool keep_world){r_hierarchy_detach(self.source,keep_world);}
static bool r_path_scene_hasComponent(RusticScenePath self,const char* name){return r_components_has(self.source,name);}
static RusticList r_path_scene_getComponents(RusticScenePath self){return r_components_get_all(self.source);}
static RusticList r_path_scene_findInChildren(RusticScenePath self,const char* name){return r_components_find_in_children(self.source,name);}
static RusticGameplayHandle r_path_scene_followPath(RusticScenePath self,RusticPath path,double duration,double speed,const char *ease,bool loop,bool pingPong,bool orient,void(*fn)(void)){return r_path_follow(self.source,path,duration,speed,ease,loop,pingPong,orient,fn);}
static void r_path_edit_attribute(RusticScenePath self,const char* name,RusticValue value){r_command("edit_attribute");r_append(",\"source\":");r_quote(self.source);r_append(",\"name\":");r_quote(name);r_append(",\"value\":");r_write_value(value);r_append("}");}
static void r_path_scene_clone(RusticScenePath self,const char* parent){r_clone(self.source,parent);}
static void r_path_scene_add(RusticScenePath self,const char* parent){r_add(self.source,parent);}
static void r_path_scene_camera(RusticScenePath self){r_camera(self.source);}
static void r_path_scene_emit(RusticScenePath self,const char* name,const RusticValue* args,size_t count){r_events_emit(name,args,count,self.source);}
static RusticScenePath r_game_scene(const char* scene,const char* path){RusticScenePath result={.source={0},.clone=r_path_scene_clone,.add=r_path_scene_add,.setCurrentCamera=r_path_scene_camera,.emit=r_path_scene_emit,.tweenTo=r_path_scene_tweenTo,.tweenMove=r_path_scene_tweenMove,.tweenRotate=r_path_scene_tweenRotate,.tweenScale=r_path_scene_tweenScale,.tweenAction=r_path_scene_tweenAction,.tweenToOptions=r_path_scene_tweenToOptions,.movementMoveTo=r_path_scene_movementMoveTo,.movementMove=r_path_scene_movementMove,.movementRotateTo=r_path_scene_movementRotateTo,.movementLookAt=r_path_scene_movementLookAt,.movementFollow=r_path_scene_movementFollow,.movementOrbit=r_path_scene_movementOrbit,.movementMoveToOptions=r_path_scene_movementMoveToOptions,.pathFollow=r_path_scene_pathFollow,.effectsFade=r_path_scene_effectsFade,.effectsFlash=r_path_scene_effectsFlash,.effectsShake=r_path_scene_effectsShake,.effectsPulse=r_path_scene_effectsPulse,.cameraMoveTo=r_path_scene_cameraMoveTo,.cameraZoom=r_path_scene_cameraZoom,.cameraFov=r_path_scene_cameraFov,.cameraFollow=r_path_scene_cameraFollow,.cameraOrbit=r_path_scene_cameraOrbit,.cameraLookAt=r_path_scene_cameraLookAt,.cameraShake=r_path_scene_cameraShake,.cameraTransition=r_path_scene_cameraTransition,.physicsImpulse=r_path_scene_physicsImpulse,.physicsForce=r_path_scene_physicsForce,.physicsLaunch=r_path_scene_physicsLaunch,.physicsKnockback=r_path_scene_physicsKnockback,.animationRegisterClip=r_path_scene_animationRegisterClip,.animationClips=r_path_scene_animationClips,.animationPlay=r_path_scene_animationPlay,.animationBlend=r_path_scene_animationBlend,.animationTransition=r_path_scene_animationTransition,.animationIk=r_path_scene_animationIk,.animationFootPlacement=r_path_scene_animationFootPlacement,.animationLookAt=r_path_scene_animationLookAt,.animationHeadTracking=r_path_scene_animationHeadTracking,.animationRecoil=r_path_scene_animationRecoil,.animationAddMarker=r_path_scene_animationAddMarker,.eventsConnect=r_path_scene_eventsConnect,.tagsAdd=r_path_scene_tagsAdd,.tagsRemove=r_path_scene_tagsRemove,.groupsAdd=r_path_scene_groupsAdd,.hierarchySetParent=r_path_scene_hierarchySetParent,.hierarchyGetChildren=r_path_scene_hierarchyGetChildren,.hierarchyDetach=r_path_scene_hierarchyDetach,.entityDestroy=r_path_scene_entityDestroy,.entityDestroyAfter=r_path_scene_entityDestroyAfter,.entityIsValid=r_path_scene_entityIsValid,.componentsHas=r_path_scene_componentsHas,.componentsGetAll=r_path_scene_componentsGetAll,.componentsFindInChildren=r_path_scene_componentsFindInChildren,.tween=r_path_scene_tween,.moveTo=r_path_scene_moveTo,.move=r_path_scene_move,.rotateTo=r_path_scene_rotateTo,.lookAt=r_path_scene_lookAt,.follow=r_path_scene_follow,.orbit=r_path_scene_orbit,.fade=r_path_scene_fade,.flash=r_path_scene_flash,.shake=r_path_scene_shake,.pulse=r_path_scene_pulse,.impulse=r_path_scene_impulse,.force=r_path_scene_force,.launch=r_path_scene_launch,.knockback=r_path_scene_knockback,.playAnimation=r_path_scene_playAnimation,.destroy=r_path_scene_destroy,.destroyAfter=r_path_scene_destroyAfter,.isValid=r_path_scene_isValid,.addTag=r_path_scene_addTag,.removeTag=r_path_scene_removeTag,.setParent=r_path_scene_setParent,.getChildren=r_path_scene_getChildren,.detach=r_path_scene_detach,.hasComponent=r_path_scene_hasComponent,.getComponents=r_path_scene_getComponents,.findInChildren=r_path_scene_findInChildren,.followPath=r_path_scene_followPath,.EditAttribute=r_path_edit_attribute};if(snprintf(result.source,sizeof result.source,"game.scene.%s.%s",scene,path)>=(int)sizeof result.source)r_fail("scene path too long");return result;}
typedef struct RusticFilePath RusticFilePath;
struct RusticFilePath {char source[4096];RusticAudioVoice(*audioPlay)(RusticFilePath self,const RusticAudioOptions *opts);RusticAudioVoice(*audioPlayAt)(RusticFilePath self,RusticVector3 position,const RusticAudioOptions *opts);RusticList(*animationLoad)(RusticFilePath self,const char *e);RusticAudioVoice(*play)(RusticFilePath self,const RusticAudioOptions *opts);RusticAudioVoice(*playAt)(RusticFilePath self,RusticVector3 position,const RusticAudioOptions *opts);RusticList(*loadAnimation)(RusticFilePath self,const char *e);void(*add)(RusticFilePath self,const char *p);void(*clone)(RusticFilePath self,const char *p);void(*setSkyTexture)(RusticFilePath self);};
static RusticAudioVoice r_path_file_audioPlay(RusticFilePath self,const RusticAudioOptions *opts){return r_audio_play(self.source,opts);}
static RusticAudioVoice r_path_file_audioPlayAt(RusticFilePath self,RusticVector3 position,const RusticAudioOptions *opts){return r_audio_at(self.source,position,opts);}
static RusticList r_path_file_animationLoad(RusticFilePath self,const char *e){return r_animation_load(e,self.source);}
static RusticAudioVoice r_path_file_play(RusticFilePath self,const RusticAudioOptions *opts){return r_audio_play(self.source,opts);}
static RusticAudioVoice r_path_file_playAt(RusticFilePath self,RusticVector3 position,const RusticAudioOptions *opts){return r_audio_at(self.source,position,opts);}
static RusticList r_path_file_loadAnimation(RusticFilePath self,const char *e){return r_animation_load(e,self.source);}
static void r_path_file_add(RusticFilePath self,const char *p){r_add(self.source,p);}
static void r_path_file_clone(RusticFilePath self,const char *p){r_clone(self.source,p);}
static void r_path_file_setSkyTexture(RusticFilePath self){r_environment_sky(self.source);}
static RusticFilePath r_game_file(const char* path){RusticFilePath result={.source={0},.audioPlay=r_path_file_audioPlay,.audioPlayAt=r_path_file_audioPlayAt,.animationLoad=r_path_file_animationLoad,.play=r_path_file_play,.playAt=r_path_file_playAt,.loadAnimation=r_path_file_loadAnimation,.add=r_path_file_add,.clone=r_path_file_clone,.setSkyTexture=r_path_file_setSkyTexture};if(snprintf(result.source,sizeof result.source,"%s",path)>=(int)sizeof result.source)r_fail("file path too long");return result;}
static const struct {RusticScenePath(*scene)(const char*,const char*);struct {RusticFilePath(*files)(const char*);RusticGameplayHandle(*tweenValue)(RusticValue from,RusticValue to,double duration,const char *ease,void(*sample)(RusticValue),void(*finished)(void));RusticGameplayHandle(*timerAfter)(double duration,void (*finished)(void));RusticGameplayHandle(*timerEvery)(double interval,void(*fn)(void),unsigned count);RusticValue(*smoothLerp)(RusticValue a,RusticValue b,double progress,const char *easing);RusticValue(*smoothSlerp)(RusticValue a,RusticValue b,double progress,const char *easing);double(*smoothInverseLerp)(double a,double b,double v);double(*smoothRemap)(double v,double a,double b,double c,double d);RusticDamping(*smoothSmoothDamp)(double current,double target,double velocity,double smoothTime,double delta);RusticHit(*physicsRaycast)(RusticVector3 origin,RusticVector3 direction,double distance);RusticHit(*physicsSphereCast)(RusticVector3 origin,RusticVector3 direction,double distance,double radius);RusticList(*physicsOverlap)(RusticVector3 center,double radius);RusticList(*physicsExplosion)(RusticVector3 center,double radius,double strength);void(*clockTimeScale)(double value);void(*clockPause)(void);void(*clockResume)(void);RusticPath(*pathCreate)(const RusticPathPoint *points,size_t count,const char *kind);RusticSequence(*sequenceNew)(void);RusticSequence(*sequenceCreate)(void);RusticSequence(*timelineNew)(void);RusticSequence(*timelineCreate)(void);RusticConnection(*eventsOn)(const char *name,void(*fn)(const RusticValue*,size_t));RusticConnection(*eventsOnce)(const char *name,void(*fn)(const RusticValue*,size_t));void(*eventsEmit)(const char *name,const RusticValue *arguments,size_t count,const char *source);void(*eventsDisconnect)(RusticConnection token);RusticAudioDevices(*audioGetOutputDevices)(void);RusticAudioDevices(*audioGetInputDevices)(void);void(*audioSetOutputDevice)(const char *device);void(*audioSetInputDevice)(const char *device);void(*audioVolume)(RusticAudioVoice v,double value);void(*audioPitch)(RusticAudioVoice v,double value);RusticGameplayHandle(*audioFadeIn)(RusticAudioVoice v,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*audioFadeOut)(RusticAudioVoice v,double duration,const char *ease,void(*fn)(void));RusticGameplayHandle(*audioCrossfade)(RusticAudioVoice a,RusticAudioVoice b,double duration,const char *ease,void(*fn)(void));void(*audioStop)(RusticAudioVoice v);void(*audioPause)(RusticAudioVoice v);void(*audioResume)(RusticAudioVoice v);RusticGameplayHandle(*animationValue)(const RusticKeyframe *keys,size_t count,void(*sample)(RusticValue),void(*finished)(void));void(*animationStop)(RusticGameplayHandle id);void(*animationPause)(RusticGameplayHandle id);void(*animationSpeed)(RusticGameplayHandle h,double speed);void(*animationLoop)(RusticGameplayHandle h,bool looping);RusticList(*tagsFindAll)(const char* name);bool(*groupsCreate)(const char* name);RusticList(*groupsGetMembers)(const char* name);RusticList(*worldFindByTag)(const char* name);RusticList(*worldFindInRadius)(RusticVector3 center,double radius);RusticGameplayHandle(*after)(double duration,void (*finished)(void));RusticGameplayHandle(*every)(double interval,void(*fn)(void),unsigned count);RusticValue(*lerp)(RusticValue a,RusticValue b,double progress,const char *easing);RusticValue(*slerp)(RusticValue a,RusticValue b,double progress,const char *easing);double(*inverseLerp)(double a,double b,double v);double(*remap)(double v,double a,double b,double c,double d);RusticDamping(*smoothDamp)(double current,double target,double velocity,double smoothTime,double delta);RusticHit(*raycast)(RusticVector3 origin,RusticVector3 direction,double distance);RusticHit(*sphereCast)(RusticVector3 origin,RusticVector3 direction,double distance,double radius);RusticList(*overlap)(RusticVector3 center,double radius);RusticList(*explosion)(RusticVector3 center,double radius,double strength);void(*timeScale)(double value);void(*pause)(void);void(*resume)(void);RusticPath(*createPath)(const RusticPathPoint *points,size_t count,const char *kind);RusticSequence(*sequence)(void);RusticSequence(*timeline)(void);RusticConnection(*on)(const char *name,void(*fn)(const RusticValue*,size_t));RusticConnection(*once)(const char *name,void(*fn)(const RusticValue*,size_t));void(*emit)(const char *name,const RusticValue *arguments,size_t count,const char *source);void(*disconnect)(RusticConnection token);RusticAudioDevices(*getOutputDevices)(void);RusticAudioDevices(*get_output_devices)(void);RusticAudioDevices(*getInputDevices)(void);RusticAudioDevices(*get_input_devices)(void);void(*setOutputDevice)(const char *device);void(*set_output_device)(const char *device);void(*setInputDevice)(const char *device);void(*set_input_device)(const char *device);const char *(*entity_id)(void);double(*delta_time)(void);double(*fixed_delta_time)(void);RusticVector3(*get_translation)(void);void(*set_translation)(double x,double y,double z);RusticValue(*get_property)(const char *n);RusticValue(*get_attribute)(const char *n);void(*set_property)(const char *n,RusticValue v);void(*edit_attribute)(const char *n,RusticValue v);RusticActionState(*input)(const char *n);RusticActionState(*key)(const char *n);RusticKeyEvents(*key_events)(void);bool(*any_key_pressed)(void);void(*log)(const char *l,const char *m);void(*set_enabled)(bool e);RusticValue(*getEnvironment)(const char *field);void(*setEnvironment)(const char *field,RusticValue value);void(*setCurrentCamera)(const char *s);} rustic;} game={r_game_scene,{r_game_file,r_tween_value,r_timer_after,r_timer_every,r_smooth_lerp,r_smooth_slerp,r_inverse_lerp,r_remap,r_smooth_damp,r_raycast,r_sphere_cast,r_overlap,r_explosion,r_clock_scale,r_clock_pause,r_clock_resume,r_path_create,r_sequence_new,r_sequence_new,r_sequence_new,r_sequence_new,r_events_on,r_events_once,r_events_emit,r_events_disconnect,r_audio_get_output_devices,r_audio_get_input_devices,r_audio_set_output_device,r_audio_set_input_device,r_audio_volume,r_audio_pitch,r_audio_fade_in,r_audio_fade_out,r_audio_crossfade,r_audio_stop,r_audio_pause,r_audio_resume,r_animation_value,r_gameplay_cancel,r_gameplay_pause,r_gameplay_speed,r_gameplay_loop,r_tags_find_all,r_groups_create,r_groups_get_members,r_world_find_by_tag,r_world_find_in_radius,r_timer_after,r_timer_every,r_smooth_lerp,r_smooth_slerp,r_inverse_lerp,r_remap,r_smooth_damp,r_raycast,r_sphere_cast,r_overlap,r_explosion,r_clock_scale,r_clock_pause,r_clock_resume,r_path_create,r_sequence_new,r_sequence_new,r_events_on,r_events_once,r_events_emit,r_events_disconnect,r_audio_get_output_devices,r_audio_get_output_devices,r_audio_get_input_devices,r_audio_get_input_devices,r_audio_set_output_device,r_audio_set_output_device,r_audio_set_input_device,r_audio_set_input_device,r_entity_id,r_delta_time,r_fixed_delta_time,r_translation,r_set_translation,r_property,r_attribute,r_set_property,r_edit_attribute,r_input,r_key,r_events,r_any,r_log,r_enabled,r_environment_get,r_environment_set,r_camera}};

static int rustic_run(RusticBehavior b) {
    /* Reference exported objects even in an empty behavior to support -Werror. */
    (void)game;(void)Capture;(void)LoadedAudio;(void)Animation;(void)Audio;(void)Events;(void)Sequence;(void)Timeline;(void)Path;(void)Effects;(void)Clock;(void)Camera;(void)Smooth;(void)Interpolation;(void)Physics;(void)rustic;(void)instance;(void)Game;(void)Tween;(void)Movement;(void)Timer;(void)Gameplay;(void)Ease;
    while(fgets(r_source,sizeof r_source,stdin)) {
        for(size_t i=0;i<r_query_owned_count;i++){free(r_query_owned[i]);}
        r_query_owned_count=0;
        if(!strchr(r_source,'\n'))r_fail("Rustic C: oversized or unterminated state");
        for(size_t i=0;i<r_count;++i){free(r_tokens[i].decoded);r_tokens[i].decoded=NULL;}
        r_count=r_at=r_written=r_commands=0;(void)r_parse(0);r_append("{\"format_version\":1,\"commands\":[");
        const char *cb=r_string(r_field(0,"callback"));double dt=r_number(r_field(0,"delta"));
        if(!strcmp(cb,"update"))r_gameplay_dispatch();
        if(!strcmp(cb,"on_create")&&b.on_create)b.on_create();else if(!strcmp(cb,"on_start")&&b.on_start)b.on_start();
        else if(!strcmp(cb,"on_enable")&&b.on_enable)b.on_enable();else if(!strcmp(cb,"on_disable")&&b.on_disable)b.on_disable();
        else if(!strcmp(cb,"on_destroy")&&b.on_destroy)b.on_destroy();else if(!strcmp(cb,"on_stop")&&b.on_stop)b.on_stop();
        else if(!strcmp(cb,"fixed_update")&&b.fixed_update)b.fixed_update(dt);else if(!strcmp(cb,"update")&&b.update)b.update(dt);
        r_append("]}");puts(r_output);fflush(stdout);
    }
    for(size_t i=0;i<r_count;++i){free(r_tokens[i].decoded);r_tokens[i].decoded=NULL;}return 0;
}
