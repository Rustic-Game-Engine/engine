
-- Runtime entity service annotations.
---@class TagsApi
---@field add fun(e:string|RusticObjectPath|nil,name:string):boolean
---@field remove fun(e:string|RusticObjectPath|nil,name:string):boolean
---@field find_all fun(name:string):string[]
Tags={}
---@class GroupsApi
---@field create fun(group:string):boolean
---@field add fun(group:string,e:string|RusticObjectPath|nil):boolean
---@field get_members fun(group:string):string[]
Groups={}
---@class HierarchyApi
---@field set_parent fun(e:string|RusticObjectPath|nil,parent:string|RusticObjectPath|nil,keep:boolean|nil)
---@field get_children fun(e:string|RusticObjectPath|nil):string[]
---@field detach fun(e:string|RusticObjectPath|nil,keep:boolean|nil)
Hierarchy={}
---@class EntityApi
---@field destroy fun(e:string|RusticObjectPath|nil)
---@field destroy_after fun(e:string|RusticObjectPath|nil,delay:number)
---@field is_valid fun(e:string|RusticObjectPath|nil):boolean
Entity={}
---@class ComponentsApi
---@field has fun(e:string|RusticObjectPath|nil,schema:string):boolean
---@field get_all fun(e:string|RusticObjectPath|nil):string[]
---@field find_in_children fun(e:string|RusticObjectPath|nil,schema:string):string[]
Components={}
---@class WorldApi
---@field find_by_tag fun(name:string):string[]
---@field find_in_radius fun(center:number[],radius:number):string[]
World={}
