import java.io.*;
import java.util.*;

class RusticBehavior {
    public static void main(String[] args) throws Exception {
        var input = new BufferedReader(new InputStreamReader(System.in));
        String request;
        var fields = List.of("callback","delta","entity_id","delta_time","fixed_delta_time",
            "translation","properties","attributes","scene_paths","actions","keys","key_events","any_key_pressed");
        while ((request = input.readLine()) != null) {
            for (var field : fields) if (!request.contains("\"" + field + "\"")) System.err.println("missing " + field);
            if (request.contains("\"callback\":\"on_start\""))
                System.out.println("{\"format_version\":1,\"commands\":[{\"op\":\"set_translation\",\"value\":[0,0,0]},{\"op\":\"set_property\",\"name\":\"smoke_value\",\"value\":1.0},{\"op\":\"edit_attribute\",\"name\":\"Position\",\"value\":[0,0,0]},{\"op\":\"log\",\"level\":\"info\",\"message\":\"Java API smoke test passed\"},{\"op\":\"set_enabled\",\"enabled\":true},{\"op\":\"add_instance\",\"source\":\"Part\",\"parent\":null},{\"op\":\"clone_instance\",\"source\":\"Part\",\"parent\":null}]}");
            else System.out.println("{\"format_version\":1,\"commands\":[]}");
            System.out.flush();
        }
    }
}

