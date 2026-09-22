export type DocEntry = { slug: string; title: string; description: string; source: string };
export type DocGroup = { label: string; docs: DocEntry[] };

export const docGroups: DocGroup[] = [
  { label: "Start here", docs: [
    { slug: "", title: "API overview", description: "Rustic API concepts, language support, and execution rules.", source: "api:overview" },
    { slug: "callbacks", title: "Lifecycle callbacks", description: "Choose the correct callback for setup, frames, physics, and teardown.", source: "api:callbacks" },
  ] },
  { label: "Core API", docs: [
    { slug: "api/entity", title: "Entity & time", description: "Read the current entity ID and frame timing.", source: "api:entity" },
    { slug: "api/transforms", title: "Transforms", description: "Read and change the owning entity's position.", source: "api:transforms" },
    { slug: "api/properties", title: "Public properties", description: "Read and update declared script properties.", source: "api:properties" },
    { slug: "api/attributes", title: "Built-in attributes", description: "Read or edit engine-owned entity attributes.", source: "api:attributes" },
    { slug: "api/input", title: "Input", description: "Use held keys in Play and see current input limits.", source: "api:input" },
    { slug: "api/logging", title: "Logging", description: "Write bounded messages to the live game console.", source: "api:logging" },
    { slug: "api/enabled", title: "Enabled state", description: "Enable or disable the owning behavior entity.", source: "api:enabled" },
  ] },
  { label: "Scene API", docs: [
    { slug: "api/scene", title: "Scene lookup", description: "Find entities and enumerate stable scene paths.", source: "api:scene" },
    { slug: "api/instances", title: "Add & clone instances", description: "Queue creation from a source path with an optional parent.", source: "api:instances" },
    { slug: "api/camera", title: "Current camera", description: "Select the active game camera by path or entity ID.", source: "api:camera" },
  ] },
  { label: "Engine guides", docs: [
    { slug: "guides/gameplay-programming", title: "Gameplay programming", description: "Script attachment, execution order, reload, and runtime behavior.", source: "docs/GAMEPLAY_PROGRAMMING.md" },
    { slug: "guides/cameras-and-lights", title: "Cameras & lights", description: "Camera components, projection, priorities, and lighting.", source: "docs/CAMERAS_AND_LIGHTS.md" },
  ] },
  { label: "Scripting languages", docs: [
    { slug: "scripting/lua", title: "Lua 5.4", description: "Create, attach, and debug Lua gameplay scripts, including a copyable keyboard controller.", source: "docs/Scripting/scriptingLua.md" },
    { slug: "scripting/javascript", title: "JavaScript", description: "Write JavaScript behaviors with lifecycle callbacks and the Script API.", source: "docs/Scripting/scriptingJavaScript.md" },
    { slug: "scripting/luau", title: "Luau", description: "Understand the current external Luau CLI adapter and its limits.", source: "docs/Scripting/scriptingLuau.md" },
    { slug: "scripting/python", title: "Python", description: "Write persistent Python protocol behaviors.", source: "docs/Scripting/scriptingPython.md" },
    { slug: "scripting/c", title: "C", description: "Build C protocol behaviors.", source: "docs/Scripting/scriptingC.md" },
    { slug: "scripting/cpp", title: "C++", description: "Build C++ protocol behaviors with the generated SDK.", source: "docs/Scripting/scriptingCpp.md" },
    { slug: "scripting/csharp", title: "C#", description: "Build C# protocol behaviors with the generated SDK.", source: "docs/Scripting/scriptingCSharp.md" },
    { slug: "scripting/java", title: "Java", description: "Build Java protocol behaviors.", source: "docs/Scripting/scriptingJava.md" },
    { slug: "scripting/php", title: "PHP", description: "Build PHP protocol behaviors for UI entries.", source: "docs/Scripting/scriptingPHP.md" },
    { slug: "scripting/web", title: "HTML/CSS", description: "Use inline JavaScript in Web script assets.", source: "docs/Scripting/scriptingWeb.md" },
  ] },
];

export const allDocs = docGroups.flatMap((group) => group.docs.map((doc) => ({ ...doc, group: group.label })));
export function routeFor(slug: string) { return slug ? `/docs/${slug}` : "/docs"; }
