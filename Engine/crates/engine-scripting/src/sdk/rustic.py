import json, sys

class InstanceApi:
    def __init__(self): self.commands = []
    def add(self, source, parent=None):
        self.commands.append({"op": "add_instance", "source": source, "parent": parent})
    def clone(self, source, parent=None):
        self.commands.append({"op": "clone_instance", "source": source, "parent": parent})

instance = InstanceApi()

class ObjectPath:
    def __init__(self, source="rustic.game"): self.source = source
    def __getattr__(self, name): return ObjectPath(self.source + "." + name)
    def __getitem__(self, name): return ObjectPath(self.source + "." + str(name))
    def EditAttribute(self, name, value):
        instance.commands.append({"op":"edit_attribute", "source":self.source, "name":name, "value":value})
    edit_attribute = EditAttribute

class RusticApi:
    game = ObjectPath()
    def __init__(self): self.state = {}; self.commands = instance.commands
    def entity_id(self): return self.state["entity_id"]
    def delta_time(self): return self.state["delta_time"]
    def fixed_delta_time(self): return self.state["fixed_delta_time"]
    def get_translation(self): return self.state["translation"][:]
    def set_translation(self, x, y, z): self.commands.append({"op":"set_translation","value":[x,y,z]})
    def get_property(self, name): return self.state["properties"].get(name)
    def set_property(self, name, value): self.commands.append({"op":"set_property","name":name,"value":value})
    def get_attribute(self, name): return self.state["attributes"].get(name)
    GetAttribute = get_attribute
    def edit_attribute(self, name, value): self.commands.append({"op":"edit_attribute","name":name,"value":value})
    EditAttribute = edit_attribute
    def input(self, name): return self.state.get("actions", {}).get(name, {"pressed":False,"released":False,"held":False,"axis":0})
    def key(self, name): return self.state["keys"].get(name, {"pressed":False,"released":False,"held":False,"axis":0})
    def key_events(self): return self.state["key_events"][:]
    def any_key_pressed(self): return self.state["any_key_pressed"]
    def log(self, level, message): self.commands.append({"op":"log","level":str(level),"message":str(message)})
    def set_enabled(self, enabled): self.commands.append({"op":"set_enabled","enabled":bool(enabled)})

class SceneApi:
    def __init__(self): self.state = {}
    def Find(self, path): return self.state.get(path)
    def List(self, path="Game.scene"):
        prefix = "" if path in ("", "Game.scene") else path.rstrip("./") + "."
        return [entity for name, entity in self.state.items() if not prefix or name.startswith(prefix)]

class GameApi:
    def setCurrentCamera(self, source):
        instance.commands.append({"op":"set_current_camera","source":source})
    set_current_camera = setCurrentCamera
rustic = RusticApi()
Game = GameApi()
Game.scene = SceneApi()

def run(callbacks):
    for line in sys.stdin:
        rustic.state = json.loads(line)
        instance.commands = []
        rustic.commands = instance.commands
        Game.scene.state = rustic.state.get("scene_paths", {})
        callback = callbacks.get(rustic.state["callback"])
        if callback:
            if rustic.state["callback"] in ("update", "fixed_update"): callback(rustic.state["delta"] or 0.0)
            else: callback()
        print(json.dumps({"format_version": 1, "commands": instance.commands}, allow_nan=False), flush=True)

