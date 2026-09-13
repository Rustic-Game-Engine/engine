#include <stdio.h>
#include <string.h>

int main(void) {
    char request[1048577];
    while (fgets(request, sizeof request, stdin)) {
        /* Touch every protocol field; a real behavior should use a JSON library. */
        const char *fields[] = {"\"callback\"","\"delta\"","\"entity_id\"","\"delta_time\"",
            "\"fixed_delta_time\"","\"translation\"","\"properties\"","\"attributes\"",
            "\"scene_paths\"","\"actions\"","\"keys\"","\"key_events\"","\"any_key_pressed\""};
        for (size_t i = 0; i < sizeof fields / sizeof fields[0]; ++i)
            if (strstr(request, fields[i]) == NULL) fprintf(stderr, "missing %s\n", fields[i]);
        if (strstr(request, "\"callback\":\"on_start\"") != NULL) {
            fputs("{\"format_version\":1,\"commands\":[{\"op\":\"set_translation\",\"value\":[0,0,0]}", stdout);
            if (strstr(request, "\"smoke_value\":") != NULL)
                fputs(",{\"op\":\"set_property\",\"name\":\"smoke_value\",\"value\":1.0}", stdout);
            puts(",{\"op\":\"edit_attribute\",\"name\":\"Position\",\"value\":[0,0,0]},{\"op\":\"log\",\"level\":\"info\",\"message\":\"C API smoke test passed\"},{\"op\":\"set_enabled\",\"enabled\":true},{\"op\":\"add_instance\",\"source\":\"Part\",\"parent\":null},{\"op\":\"clone_instance\",\"source\":\"Part\",\"parent\":null}]}");
        } else puts("{\"format_version\":1,\"commands\":[]}");
        fflush(stdout);
    }
    return 0;
}

