// Runtime entity services. Source: the shared engine-world/query boundary.
declare const Tags: {
  add(e: string | null | RusticObjectPath, name: string): boolean;
  remove(e: string | null | RusticObjectPath, name: string): boolean;
  find_all(name: string): string[];
};
declare const Groups: {
  create(group: string): boolean;
  add(group: string, e: string | null | RusticObjectPath): boolean;
  get_members(group: string): string[];
};
declare const Hierarchy: {
  set_parent(e: string | null | RusticObjectPath, parent: string | null | RusticObjectPath, keep?: boolean): void;
  get_children(e: string | null | RusticObjectPath): string[];
  detach(e: string | null | RusticObjectPath, keep?: boolean): void;
};
declare const Entity: {
  destroy(e: string | null | RusticObjectPath): void;
  destroy_after(e: string | null | RusticObjectPath, delay: number): void;
  is_valid(e: string | null | RusticObjectPath): boolean;
};
declare const Components: {
  has(e: string | null | RusticObjectPath, schema: string): boolean;
  get_all(e: string | null | RusticObjectPath): string[];
  find_in_children(e: string | null | RusticObjectPath, schema: string): string[];
};
declare const World: {
  find_by_tag(name: string): string[];
  find_in_radius(center: [number, number, number], radius: number): string[];
};
