#include <array>
#include <iostream>
#include <string>

int main() {
    std::string request;
    while (std::getline(std::cin, request)) {
        const std::array fields{"\"callback\"","\"delta\"","\"entity_id\"","\"delta_time\"",
            "\"fixed_delta_time\"","\"translation\"","\"properties\"","\"attributes\"",
            "\"scene_paths\"","\"actions\"","\"keys\"","\"key_events\"","\"any_key_pressed\""};
        for (const auto *field : fields)
            if (!request.contains(field)) std::cerr << "missing " << field << '\n';
        if (request.contains("\"callback\":\"on_start\"")) {
            std::cout << R"({"format_version":1,"commands":[{"op":"set_translation","value":[0,0,0]})";
            if (request.contains("\"smoke_value\":"))
                std::cout << R"(,{"op":"set_property","name":"smoke_value","value":1.0})";
            std::cout << R"(,{"op":"edit_attribute","name":"Position","value":[0,0,0]},{"op":"log","level":"info","message":"C++ API smoke test passed"},{"op":"set_enabled","enabled":true},{"op":"add_instance","source":"Part","parent":null},{"op":"clone_instance","source":"Part","parent":null}]})" << std::endl;
        }
        else std::cout << R"({"format_version":1,"commands":[]})" << std::endl;
    }
}

